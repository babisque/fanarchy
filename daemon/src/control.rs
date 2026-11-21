//! Control loop and restore logic.
//!
//! Every tick: read sensors → [`curve`] → [`safety::clamp`] → write `pwmN`,
//! only when the duty changed. Before the first write, the original
//! `pwmN_enable` of each controlled output is saved in a [`Restorer`],
//! which puts it back on shutdown, error or panic.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fanarchy_protocol::{FanMode, Request, Response};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::MissedTickBehavior;

use crate::config::{
    self, ChangeError, Config, ControlledFan, Plan, ResolveError, ResolvedSensor, SafetySettings,
};
use crate::curve::{self, CurvePoint};
use crate::hwmon::{Chip, Hwmon, HwmonError, OutputRef, PwmMode};
use crate::safety::{self, Duty, Override};

/// Time between two control ticks.
pub const TICK: Duration = Duration::from_secs(1);

/// Consecutive ticks with hardware errors before the daemon gives up
/// (and restores automatic control).
pub const MAX_FAILED_TICKS: u32 = 5;

/// The `pwmN_enable` value for automatic (BIOS) control.
pub const AUTOMATIC: PwmMode = PwmMode::Automatic(2);

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error(transparent)]
    Hwmon(#[from] HwmonError),
    #[error("config does not match the hardware:\n{}", bullet_list(.0))]
    Resolve(Vec<ResolveError>),
    #[error("fan '{fan}': cannot take control of the output: {source}")]
    Takeover {
        fan: String,
        #[source]
        source: HwmonError,
    },
    #[error("hardware errors for {0} ticks in a row, giving up")]
    TooManyFailures(u32),
}

fn bullet_list<T: std::fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(|item| format!("  - {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Read the current temperature of a resolved sensor.
/// `None` when the chip or sensor is gone or the reading is invalid.
pub fn read_sensor(chips: &[Chip], sensor: &ResolvedSensor) -> Option<f32> {
    chips
        .iter()
        .find(|chip| chip.path == sensor.chip_path)?
        .temps
        .iter()
        .find(|temp| temp.index == sensor.index)?
        .celsius
}

/// Per-fan memory between ticks.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FanState {
    /// Temperature the curve was last evaluated at (hysteresis anchor).
    pub anchor_c: Option<f32>,
    /// Speed applied on the last tick, in %.
    pub percent: Option<f32>,
    /// Override active on the last tick.
    pub over: Override,
}

/// One control step for one fan. Pure.
///
/// Curve (with hysteresis and smoothing) first, then the safety rules.
/// Overrides skip smoothing and apply immediately. The hysteresis anchor
/// keeps following the sensor during an override, so the curve is
/// current once the override clears.
pub fn step(
    fan: &ControlledFan,
    safety: &SafetySettings,
    previous: &FanState,
    cpu_temp_c: Option<f32>,
    fan_temp_c: Option<f32>,
) -> (FanState, Duty) {
    let over = safety::evaluate_override(cpu_temp_c, fan_temp_c, safety.critical_temp_c);
    let anchor_c = next_anchor(fan, previous, fan_temp_c);
    let requested = match (over, anchor_c) {
        (Override::None, Some(anchor)) => curve::smooth(
            previous.percent,
            curve::evaluate(&fan.curve, anchor),
            fan.step_up_percent,
            fan.step_down_percent,
        ),
        // Overrides replace the requested value in `clamp`.
        _ => 100.0,
    };
    let duty = safety::clamp(requested, fan.min_percent, over, safety.sensor_fail_percent);
    let state = FanState {
        anchor_c,
        percent: Some(duty.percent()),
        over,
    };
    (state, duty)
}

/// The hysteresis anchor keeps following the sensor whatever the mode,
/// so the curve is current when it takes over again.
fn next_anchor(fan: &ControlledFan, previous: &FanState, fan_temp_c: Option<f32>) -> Option<f32> {
    match fan_temp_c {
        Some(temp) => Some(curve::apply_hysteresis(
            previous.anchor_c,
            temp,
            fan.hysteresis_c,
        )),
        None => previous.anchor_c,
    }
}

/// How a fan is driven, set over IPC. Runtime only: never saved, and
/// every fan starts in [`Mode::Curve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Curve,
    /// A fixed speed in %, still raised to `min_percent`.
    Fixed(u8),
    /// Motherboard control, except while a safety override is active.
    Auto,
}

impl Mode {
    pub fn protocol(self) -> FanMode {
        match self {
            Mode::Curve => FanMode::Curve,
            Mode::Fixed(_) => FanMode::Fixed,
            Mode::Auto => FanMode::Auto,
        }
    }
}

/// Check a `set_mode` request for one fan.
pub fn parse_mode(
    fan: &ControlledFan,
    mode: FanMode,
    percent: Option<u8>,
) -> Result<Mode, ChangeError> {
    match (mode, percent) {
        (FanMode::Fixed, None) => Err(ChangeError::PercentRequired),
        (FanMode::Fixed, Some(value)) if !(fan.min_percent..=100).contains(&value) => {
            Err(ChangeError::PercentOutOfRange {
                fan: fan.id.clone(),
                value,
                min: fan.min_percent,
            })
        }
        (FanMode::Fixed, Some(value)) => Ok(Mode::Fixed(value)),
        (FanMode::Curve | FanMode::Auto, Some(_)) => Err(ChangeError::PercentNotAllowed),
        (FanMode::Curve, None) => Ok(Mode::Curve),
        (FanMode::Auto, None) => Ok(Mode::Auto),
    }
}

/// What to do with a fan's output this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Drive the fan at this duty (manual mode).
    Write(Duty),
    /// Hand the fan to the motherboard (`pwmN_enable = 2`).
    Release,
}

/// One control step for one fan in any [`Mode`]. Pure.
///
/// Safety overrides apply in every mode: a critical CPU or a failed
/// sensor always drives the fan through [`safety::clamp`], even in
/// [`Mode::Auto`].
pub fn step_mode(
    fan: &ControlledFan,
    safety: &SafetySettings,
    previous: &FanState,
    mode: Mode,
    cpu_temp_c: Option<f32>,
    fan_temp_c: Option<f32>,
) -> (FanState, Action) {
    let over = safety::evaluate_override(cpu_temp_c, fan_temp_c, safety.critical_temp_c);
    let anchor_c = next_anchor(fan, previous, fan_temp_c);
    let requested = match mode {
        Mode::Curve => {
            let (state, duty) = step(fan, safety, previous, cpu_temp_c, fan_temp_c);
            return (state, Action::Write(duty));
        }
        Mode::Fixed(percent) => f32::from(percent),
        Mode::Auto if over == Override::None => {
            let state = FanState {
                anchor_c,
                percent: None,
                over,
            };
            return (state, Action::Release);
        }
        // Overrides replace the requested value in `clamp`.
        Mode::Auto => 100.0,
    };
    let duty = safety::clamp(requested, fan.min_percent, over, safety.sensor_fail_percent);
    let state = FanState {
        anchor_c,
        percent: Some(duty.percent()),
        over,
    };
    (state, Action::Write(duty))
}

#[derive(Debug, Clone, Copy)]
struct FanRuntime {
    state: FanState,
    mode: Mode,
    /// Last duty successfully written. `None` forces a write.
    written: Option<Duty>,
    /// The output is handed to the motherboard (`pwmN_enable = 2`).
    released: bool,
    /// Last reading of the fan's sensor.
    temp_c: Option<f32>,
}

/// One controlled fan in a [`ControlSnapshot`].
#[derive(Debug, Clone, PartialEq)]
pub struct FanSnapshot {
    pub id: String,
    pub mode: Mode,
    pub over: Override,
    /// `None` while the motherboard drives the fan.
    pub percent: Option<f32>,
    pub temp_c: Option<f32>,
}

/// What the controller did on its last tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ControlSnapshot {
    pub cpu_temp_c: Option<f32>,
    pub fans: Vec<FanSnapshot>,
    /// Every hwmon reading from the last tick.
    pub chips: Vec<Chip>,
}

/// Drives the controlled fans of a [`Plan`].
pub struct Controller<H: Hwmon> {
    hwmon: Arc<H>,
    plan: Plan,
    fans: Vec<FanRuntime>,
    failed_ticks: u32,
    cpu_temp_c: Option<f32>,
    chips: Vec<Chip>,
}

impl<H: Hwmon> Controller<H> {
    /// Take control of every output in the plan.
    ///
    /// For each output: write the target `pwmN`, switch `pwmN_enable` to
    /// manual, then write `pwmN` again. Some it87 chips reject `pwmN`
    /// while in automatic mode; that first write failing is logged and
    /// tolerated, since the register then still holds the BIOS duty.
    pub fn start(hwmon: Arc<H>, plan: Plan) -> Result<Self, ControlError> {
        let chips = hwmon.chips()?;
        let cpu = read_sensor(&chips, &plan.cpu_sensor);
        let mut fans = Vec::with_capacity(plan.fans.len());
        for fan in &plan.fans {
            let temp = read_sensor(&chips, &fan.sensor);
            let (state, duty) = step(fan, &plan.safety, &FanState::default(), cpu, temp);
            log_override_change(fan, &plan, Override::None, state.over, cpu, temp);
            take_control(hwmon.as_ref(), fan, duty)?;
            fans.push(FanRuntime {
                state,
                mode: Mode::Curve,
                written: Some(duty),
                released: false,
                temp_c: temp,
            });
        }
        Ok(Self {
            hwmon,
            plan,
            fans,
            failed_ticks: 0,
            cpu_temp_c: cpu,
            chips,
        })
    }

    /// Run one control step for every fan.
    ///
    /// Hardware errors are logged and retried on the next tick. After
    /// [`MAX_FAILED_TICKS`] failing ticks in a row this returns an error.
    pub fn tick(&mut self) -> Result<(), ControlError> {
        let (chips, mut healthy) = match self.hwmon.chips() {
            Ok(chips) => (chips, true),
            Err(err) => {
                tracing::warn!(%err, "cannot read hwmon, treating every sensor as failed");
                (Vec::new(), false)
            }
        };
        let cpu = read_sensor(&chips, &self.plan.cpu_sensor);

        for (fan, runtime) in self.plan.fans.iter().zip(self.fans.iter_mut()) {
            let temp = read_sensor(&chips, &fan.sensor);
            let (state, action) = step_mode(
                fan,
                &self.plan.safety,
                &runtime.state,
                runtime.mode,
                cpu,
                temp,
            );
            log_override_change(fan, &self.plan, runtime.state.over, state.over, cpu, temp);
            runtime.state = state;
            runtime.temp_c = temp;
            healthy &= apply(self.hwmon.as_ref(), &self.plan, fan, runtime, action);
        }
        self.cpu_temp_c = cpu;
        self.chips = chips;

        if healthy {
            self.failed_ticks = 0;
            return Ok(());
        }
        self.failed_ticks += 1;
        if self.failed_ticks >= MAX_FAILED_TICKS {
            return Err(ControlError::TooManyFailures(self.failed_ticks));
        }
        Ok(())
    }
}

impl<H: Hwmon> Controller<H> {
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// The state after the last tick.
    pub fn snapshot(&self) -> ControlSnapshot {
        let fans = self
            .plan
            .fans
            .iter()
            .zip(&self.fans)
            .map(|(fan, runtime)| FanSnapshot {
                id: fan.id.clone(),
                mode: runtime.mode,
                over: runtime.state.over,
                percent: if runtime.released {
                    None
                } else {
                    runtime.state.percent
                },
                temp_c: runtime.temp_c,
            })
            .collect();
        ControlSnapshot {
            cpu_temp_c: self.cpu_temp_c,
            fans,
            chips: self.chips.clone(),
        }
    }

    fn index_of(&self, id: &str) -> Result<usize, ChangeError> {
        self.plan
            .fans
            .iter()
            .position(|fan| fan.id == id)
            .ok_or_else(|| ChangeError::UnknownFan(id.to_string()))
    }

    /// The controlled fan with this id.
    pub fn fan(&self, id: &str) -> Result<&ControlledFan, ChangeError> {
        let index = self.index_of(id)?;
        Ok(&self.plan.fans[index])
    }

    /// Replace a fan's curve, from the next tick on. The curve must
    /// already be validated.
    pub fn set_curve(&mut self, id: &str, curve: Vec<CurvePoint>) -> Result<(), ChangeError> {
        let index = self.index_of(id)?;
        self.plan.fans[index].curve = curve;
        Ok(())
    }

    /// Switch a fan's mode, from the next tick on.
    pub fn set_mode(&mut self, id: &str, mode: Mode) -> Result<(), ChangeError> {
        let index = self.index_of(id)?;
        self.fans[index].mode = mode;
        Ok(())
    }

    /// Match `config` against the hardware, as at startup.
    pub fn resolve(&self, config: &Config) -> Result<Plan, ControlError> {
        let chips = self.hwmon.chips()?;
        config::resolve(config, &chips).map_err(ControlError::Resolve)
    }

    /// Switch to a plan controlling the same outputs (see
    /// [`config::same_outputs`]). Every fan goes back to [`Mode::Curve`];
    /// the rest of its state is kept, matched by output.
    pub fn replace_plan(&mut self, plan: Plan) {
        let fans = plan
            .fans
            .iter()
            .map(|fan| {
                let old = self
                    .plan
                    .fans
                    .iter()
                    .position(|old| old.output == fan.output)
                    .map(|index| self.fans[index]);
                match old {
                    Some(runtime) => FanRuntime {
                        mode: Mode::Curve,
                        ..runtime
                    },
                    // Not reachable with the same outputs; force a takeover.
                    None => FanRuntime {
                        state: FanState::default(),
                        mode: Mode::Curve,
                        written: None,
                        released: true,
                        temp_c: None,
                    },
                }
            })
            .collect();
        self.fans = fans;
        self.plan = plan;
    }
}

/// Carry out one fan's [`Action`]. Returns `false` on a hardware error.
fn apply<H: Hwmon>(
    hwmon: &H,
    plan: &Plan,
    fan: &ControlledFan,
    runtime: &mut FanRuntime,
    action: Action,
) -> bool {
    match action {
        Action::Write(duty) if runtime.released => {
            if !in_plan(plan, fan) {
                return true;
            }
            match take_control(hwmon, fan, duty) {
                Ok(()) => {
                    runtime.released = false;
                    runtime.written = Some(duty);
                    true
                }
                Err(err) => {
                    tracing::warn!(fan = %fan.id, %err, "cannot take the fan back, retrying next tick");
                    runtime.written = None;
                    false
                }
            }
        }
        Action::Write(duty) => {
            if runtime.written == Some(duty) || !in_plan(plan, fan) {
                return true;
            }
            match hwmon.write_pwm(&fan.output, duty) {
                Ok(()) => {
                    runtime.written = Some(duty);
                    true
                }
                Err(err) => {
                    tracing::warn!(fan = %fan.id, %err, "pwm write failed, retrying next tick");
                    runtime.written = None;
                    false
                }
            }
        }
        Action::Release => {
            if runtime.released || !in_plan(plan, fan) {
                return true;
            }
            match hwmon.write_enable(&fan.output, AUTOMATIC) {
                Ok(()) => {
                    tracing::info!(fan = %fan.id, "fan handed to the motherboard (auto mode)");
                    runtime.released = true;
                    runtime.written = None;
                    true
                }
                Err(err) => {
                    tracing::warn!(fan = %fan.id, %err, "cannot hand the fan to the motherboard, retrying next tick");
                    false
                }
            }
        }
    }
}

/// Defence in depth: only outputs from the validated plan are written.
fn in_plan(plan: &Plan, fan: &ControlledFan) -> bool {
    let ok = plan.controls(&fan.output);
    if !ok {
        tracing::error!(fan = %fan.id, "refusing to write an output outside the plan");
    }
    ok
}

fn take_control<H: Hwmon>(hwmon: &H, fan: &ControlledFan, duty: Duty) -> Result<(), ControlError> {
    match hwmon.write_pwm(&fan.output, duty) {
        Ok(()) => {}
        Err(err) if err.is_busy() => tracing::debug!(
            fan = %fan.id,
            "pwm pre-write rejected with EBUSY (driver refuses pwm writes in automatic mode), switching to manual"
        ),
        Err(err) => tracing::warn!(
            fan = %fan.id,
            %err,
            "pwm pre-write rejected, switching to manual anyway"
        ),
    }
    let takeover = |source| ControlError::Takeover {
        fan: fan.id.clone(),
        source,
    };
    hwmon
        .write_enable(&fan.output, PwmMode::Manual)
        .map_err(takeover)?;
    hwmon.write_pwm(&fan.output, duty).map_err(takeover)?;
    tracing::info!(fan = %fan.id, percent = duty.percent().round(), "fan under control");
    Ok(())
}

fn log_override_change(
    fan: &ControlledFan,
    plan: &Plan,
    before: Override,
    after: Override,
    cpu: Option<f32>,
    temp: Option<f32>,
) {
    if before == after {
        return;
    }
    match after {
        Override::Critical => tracing::warn!(
            fan = %fan.id,
            cpu = ?cpu,
            limit = plan.safety.critical_temp_c,
            "CPU above critical limit, forcing 100 %"
        ),
        Override::SensorFailed => tracing::warn!(
            fan = %fan.id,
            sensor = %fan.sensor.name,
            cpu_sensor = %plan.cpu_sensor.name,
            cpu = ?cpu,
            temp = ?temp,
            percent = plan.safety.sensor_fail_percent,
            "sensor unreadable, forcing sensor_fail_percent"
        ),
        Override::None => tracing::warn!(fan = %fan.id, "safety override cleared, back to curve"),
    }
}

/// The mode to restore for an original `pwmN_enable`.
///
/// Manual (`1`) becomes automatic: restoring manual would leave the fan
/// stuck at its last duty with nothing controlling it.
pub fn restore_mode(original: PwmMode) -> PwmMode {
    match original {
        PwmMode::Manual => AUTOMATIC,
        other => other,
    }
}

/// Write `mode` to `pwmN_enable`; if that fails, drive the fan to full
/// speed instead. Returns whether the mode was restored.
pub fn restore_output<H: Hwmon>(hwmon: &H, fan: &str, output: &OutputRef, mode: PwmMode) -> bool {
    match hwmon.write_enable(output, mode) {
        Ok(()) => {
            tracing::warn!(fan, mode = mode.raw(), "restored pwm_enable");
            true
        }
        Err(err) => {
            tracing::error!(fan, %err, "cannot restore pwm_enable, falling back to full speed");
            if let Err(err) = hwmon.write_pwm(output, Duty::FULL) {
                tracing::error!(fan, %err, "full speed fallback failed too");
            }
            false
        }
    }
}

/// Puts every controlled `pwmN_enable` back to its original value.
/// Runs at most once, whoever calls it first.
pub struct Restorer<H: Hwmon> {
    hwmon: Arc<H>,
    outputs: Vec<(String, OutputRef, PwmMode)>,
    done: AtomicBool,
}

impl<H: Hwmon> Restorer<H> {
    pub fn new(hwmon: Arc<H>, plan: &Plan) -> Self {
        let outputs = plan
            .fans
            .iter()
            .map(|fan| {
                if fan.original_mode == PwmMode::Manual {
                    tracing::warn!(
                        fan = %fan.id,
                        "original pwm_enable is manual (1), will restore automatic (2) instead"
                    );
                }
                (
                    fan.id.clone(),
                    fan.output.clone(),
                    restore_mode(fan.original_mode),
                )
            })
            .collect();
        Self {
            hwmon,
            outputs,
            done: AtomicBool::new(false),
        }
    }

    /// Restore every output. Returns `false` if any could not be restored.
    /// Later calls do nothing and return `true`.
    pub fn restore(&self) -> bool {
        if self.done.swap(true, Ordering::SeqCst) {
            return true;
        }
        let mut all_ok = true;
        for (fan, output, mode) in &self.outputs {
            all_ok &= restore_output(self.hwmon.as_ref(), fan, output, *mode);
        }
        all_ok
    }
}

/// Restores on drop: covers normal returns, `?` errors and unwinding.
pub struct RestoreGuard<H: Hwmon>(pub Arc<Restorer<H>>);

impl<H: Hwmon> Drop for RestoreGuard<H> {
    fn drop(&mut self) {
        self.0.restore();
    }
}

/// Restore from the panic hook, before the default hook prints the panic.
pub fn install_panic_hook<H>(restorer: Arc<Restorer<H>>)
where
    H: Hwmon + Send + Sync + 'static,
{
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restorer.restore();
        previous(info);
    }));
}

/// Whether [`run`] installs the panic hook. Tests skip it because the
/// hook is process-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanicHook {
    Install,
    Skip,
}

/// Run the daemon until `shutdown` completes or an error occurs.
///
/// Every exit path restores the original `pwmN_enable` values.
pub async fn run<H, F>(
    hwmon: Arc<H>,
    config: &Config,
    panic_hook: PanicHook,
    shutdown: F,
) -> Result<(), ControlError>
where
    H: Hwmon + Send + Sync + 'static,
    F: Future<Output = ()>,
{
    serve(hwmon, config.clone(), panic_hook, shutdown, None).await
}

/// A request from an IPC client, with the channel for its reply.
#[derive(Debug)]
pub struct ControlRequest {
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

/// What the control loop publishes after every tick and change.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    /// The config in use, as saved on disk.
    pub config: Config,
    pub snapshot: ControlSnapshot,
}

/// The control loop's side of the IPC channel.
pub struct Service {
    /// Where `set_curve` saves and `reload` reads.
    pub config_path: PathBuf,
    pub requests: mpsc::Receiver<ControlRequest>,
    pub published: watch::Sender<Option<Arc<Published>>>,
}

/// [`run`], also serving IPC requests when `service` is given.
///
/// The control loop owns the controller and the config; IPC clients only
/// reach them through [`Service::requests`], one request at a time.
pub async fn serve<H, F>(
    hwmon: Arc<H>,
    config: Config,
    panic_hook: PanicHook,
    shutdown: F,
    service: Option<Service>,
) -> Result<(), ControlError>
where
    H: Hwmon + Send + Sync + 'static,
    F: Future<Output = ()>,
{
    let chips = hwmon.chips()?;
    let plan = config::resolve(&config, &chips).map_err(ControlError::Resolve)?;

    let restorer = Arc::new(Restorer::new(Arc::clone(&hwmon), &plan));
    if panic_hook == PanicHook::Install {
        install_panic_hook(Arc::clone(&restorer));
    }
    let _guard = RestoreGuard(Arc::clone(&restorer));

    let mut controller = Controller::start(hwmon, plan)?;
    let mut state = Daemon { config, service };
    let result = run_loop(&mut controller, &mut state, shutdown).await;
    if let Err(err) = &result {
        tracing::error!(%err, "control loop stopped");
    }
    restorer.restore();
    result
}

/// The control loop's state besides the controller.
struct Daemon {
    config: Config,
    service: Option<Service>,
}

impl Daemon {
    fn publish<H: Hwmon>(&self, controller: &Controller<H>) {
        if let Some(service) = &self.service {
            service.published.send_replace(Some(Arc::new(Published {
                config: self.config.clone(),
                snapshot: controller.snapshot(),
            })));
        }
    }

    async fn next_request(&mut self) -> Option<ControlRequest> {
        match &mut self.service {
            Some(service) => service.requests.recv().await,
            None => std::future::pending().await,
        }
    }
}

/// Tick every [`TICK`] until `shutdown` completes, serving requests in
/// between. A change applied over IPC triggers a tick right away.
async fn run_loop<H, F>(
    controller: &mut Controller<H>,
    state: &mut Daemon,
    shutdown: F,
) -> Result<(), ControlError>
where
    H: Hwmon,
    F: Future<Output = ()>,
{
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // The first tick completes at once; `start` already applied the curve.
    interval.tick().await;
    state.publish(controller);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => {
                tracing::info!("shutdown requested");
                return Ok(());
            }
            _ = interval.tick() => {
                controller.tick()?;
                state.publish(controller);
            }
            Some(ControlRequest { request, reply }) = state.next_request() => {
                let response = match handle_request(controller, state, request) {
                    Ok(response) => {
                        interval.reset_immediately();
                        response
                    }
                    Err(message) => Response::error(message),
                };
                state.publish(controller);
                // The client may have gone away; nothing to do then.
                let _ = reply.send(response);
            }
        }
    }
}

/// Apply one change request. Nothing changes when it returns an error.
fn handle_request<H: Hwmon>(
    controller: &mut Controller<H>,
    state: &mut Daemon,
    request: Request,
) -> Result<Response, String> {
    let Some(service) = &state.service else {
        return Err("IPC is not enabled".to_string());
    };
    match request {
        Request::SetCurve { fan, points } => {
            let changed = config::with_curve(&state.config, &fan, points.clone())
                .map_err(|e| e.to_string())?;
            controller.fan(&fan).map_err(|e| e.to_string())?;
            config::save_atomic(&service.config_path, &changed).map_err(|e| e.one_line())?;
            controller
                .set_curve(&fan, points)
                .map_err(|e| e.to_string())?;
            state.config = changed;
            tracing::info!(fan = %fan, "curve updated and saved");
            Ok(Response::ok_empty())
        }
        Request::SetMode { fan, mode, percent } => {
            config::controllable_fan(&state.config, &fan).map_err(|e| e.to_string())?;
            let controlled = controller.fan(&fan).map_err(|e| e.to_string())?;
            let mode = parse_mode(controlled, mode, percent).map_err(|e| e.to_string())?;
            controller.set_mode(&fan, mode).map_err(|e| e.to_string())?;
            tracing::info!(fan = %fan, ?mode, "mode changed");
            Ok(Response::ok_empty())
        }
        Request::Reload => {
            let config = config::load(&service.config_path).map_err(|e| e.one_line())?;
            let plan = controller.resolve(&config).map_err(|e| match e {
                ControlError::Resolve(errors) => format!(
                    "config does not match the hardware: {}",
                    errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                other => other.to_string(),
            })?;
            if !config::same_outputs(controller.plan(), &plan) {
                return Err(
                    "controlled outputs changed; restart the daemon (sudo systemctl restart fanarchyd)"
                        .to_string(),
                );
            }
            for warning in config::warnings(&config) {
                tracing::warn!("{warning}");
            }
            controller.replace_plan(plan);
            state.config = config;
            tracing::info!("config reloaded");
            Ok(Response::ok_empty())
        }
        Request::Status | Request::GetConfig | Request::Subscribe => {
            Err("this request is answered by the socket server".to_string())
        }
    }
}

/// Result of `fanarchyd restore`.
#[derive(Debug, Default)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub failed: Vec<String>,
    pub errors: Vec<ResolveError>,
}

impl RestoreReport {
    pub fn is_ok(&self) -> bool {
        self.failed.is_empty() && self.errors.is_empty()
    }
}

/// Set `pwmN_enable` to automatic for every `role: fan` output in the
/// config. Needs no saved state, so it works after SIGKILL or OOM (it is
/// the systemd `ExecStopPost` hook). Pumps are never touched.
pub fn restore_from_config<H: Hwmon>(
    hwmon: &H,
    config: &Config,
) -> Result<RestoreReport, HwmonError> {
    let chips = hwmon.chips()?;
    let (outputs, errors) = config::restore_outputs(config, &chips);
    let mut report = RestoreReport {
        errors,
        ..RestoreReport::default()
    };
    for (fan, output) in outputs {
        if restore_output(hwmon, &fan, &output, AUTOMATIC) {
            report.restored.push(fan);
        } else {
            report.failed.push(fan);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fanarchy_protocol::FanMode;

    use super::*;

    fn fan() -> ControlledFan {
        ControlledFan {
            id: "case".into(),
            output: OutputRef {
                chip_path: PathBuf::from("/fake/hwmon5"),
                index: 1,
            },
            original_mode: AUTOMATIC,
            sensor: ResolvedSensor {
                name: "cpu/temp1".into(),
                chip_path: PathBuf::from("/fake/hwmon2"),
                index: 1,
            },
            curve: vec![(40.0, 30.0), (60.0, 50.0), (80.0, 100.0)],
            min_percent: 20,
            hysteresis_c: 2.0,
            step_up_percent: 10.0,
            step_down_percent: 3.0,
        }
    }

    const SAFETY: SafetySettings = SafetySettings {
        critical_temp_c: 85.0,
        sensor_fail_percent: 100,
    };

    fn run_steps(temps: &[(Option<f32>, Option<f32>)]) -> Vec<(u8, Override)> {
        let fan = fan();
        let mut state = FanState::default();
        temps
            .iter()
            .map(|(cpu, temp)| {
                let (next, duty) = step(&fan, &SAFETY, &state, *cpu, *temp);
                state = next;
                (duty.raw(), state.over)
            })
            .collect()
    }

    #[test]
    fn first_step_applies_curve_directly() {
        // 50 °C → 40 % → 102.
        assert_eq!(
            run_steps(&[(Some(50.0), Some(50.0))]),
            [(102, Override::None)]
        );
    }

    #[test]
    fn rising_temperature_ramps_up() {
        // 40 °C → 30 % (pwm 77 = 30.2 %), then 80 °C wants 100 %:
        // +10 % per tick from the applied duty → 40.2 %, 50.2 %.
        let got = run_steps(&[
            (Some(40.0), Some(40.0)),
            (Some(80.0), Some(80.0)),
            (Some(80.0), Some(80.0)),
        ]);
        let raw: Vec<u8> = got.iter().map(|(raw, _)| *raw).collect();
        assert_eq!(raw, [77, 103, 129]);
    }

    #[test]
    fn hysteresis_holds_small_drops() {
        let got = run_steps(&[
            (Some(60.0), Some(60.0)),
            (Some(59.0), Some(59.0)),
            (Some(58.5), Some(58.5)),
        ]);
        let raw: Vec<u8> = got.iter().map(|(raw, _)| *raw).collect();
        assert_eq!(raw, [128, 128, 128]);
    }

    #[test]
    fn critical_jumps_to_full_and_ramps_down_after() {
        let got = run_steps(&[
            (Some(50.0), Some(50.0)),
            (Some(90.0), Some(50.0)),
            (Some(50.0), Some(50.0)),
        ]);
        assert_eq!(got[1], (255, Override::Critical));
        // Back to curve: 100 % - 3 % step.
        assert_eq!(got[2], (247, Override::None));
    }

    #[test]
    fn failed_sensor_goes_full_immediately() {
        let got = run_steps(&[(Some(45.0), Some(45.0)), (Some(45.0), None)]);
        assert_eq!(got[1], (255, Override::SensorFailed));
    }

    #[test]
    fn failed_cpu_sensor_goes_full() {
        let got = run_steps(&[(None, Some(45.0))]);
        assert_eq!(got[0], (255, Override::SensorFailed));
    }

    #[test]
    fn floor_applies_below_curve() {
        // Curve says 30 % but a 40 % minimum wins.
        let mut fan = fan();
        fan.min_percent = 40;
        let (_, duty) = step(&fan, &SAFETY, &FanState::default(), Some(30.0), Some(30.0));
        assert_eq!(duty.raw(), 102);
    }

    fn mode_step(mode: Mode, cpu: Option<f32>, temp: Option<f32>) -> (FanState, Action) {
        step_mode(&fan(), &SAFETY, &FanState::default(), mode, cpu, temp)
    }

    fn written(action: Action) -> u8 {
        match action {
            Action::Write(duty) => duty.raw(),
            Action::Release => panic!("expected a write, got a release"),
        }
    }

    #[test]
    fn curve_mode_matches_step() {
        let (state, duty) = step(
            &fan(),
            &SAFETY,
            &FanState::default(),
            Some(50.0),
            Some(50.0),
        );
        assert_eq!(
            mode_step(Mode::Curve, Some(50.0), Some(50.0)),
            (state, Action::Write(duty))
        );
    }

    #[test]
    fn fixed_mode_applies_safety_rules() {
        // (mode, cpu, fan temp, expected raw pwm, expected override)
        let cases = [
            (Mode::Fixed(60), Some(50.0), Some(50.0), 153, Override::None),
            // Below the 20 % minimum: raised to it.
            (Mode::Fixed(10), Some(50.0), Some(50.0), 51, Override::None),
            (
                Mode::Fixed(60),
                Some(90.0),
                Some(50.0),
                255,
                Override::Critical,
            ),
            (
                Mode::Fixed(60),
                Some(50.0),
                None,
                255,
                Override::SensorFailed,
            ),
        ];
        for (mode, cpu, temp, raw, over) in cases {
            let (state, action) = mode_step(mode, cpu, temp);
            assert_eq!(written(action), raw, "{mode:?} {cpu:?} {temp:?}");
            assert_eq!(state.over, over);
        }
    }

    #[test]
    fn fixed_mode_does_not_smooth() {
        let previous = FanState {
            anchor_c: Some(50.0),
            percent: Some(30.0),
            over: Override::None,
        };
        let (_, action) = step_mode(
            &fan(),
            &SAFETY,
            &previous,
            Mode::Fixed(100),
            Some(50.0),
            Some(50.0),
        );
        assert_eq!(written(action), 255);
    }

    #[test]
    fn auto_mode_releases_unless_overridden() {
        let (state, action) = mode_step(Mode::Auto, Some(50.0), Some(50.0));
        assert_eq!(action, Action::Release);
        assert_eq!(state.percent, None);
        assert_eq!(state.anchor_c, Some(50.0));

        let (state, action) = mode_step(Mode::Auto, Some(90.0), Some(50.0));
        assert_eq!((written(action), state.over), (255, Override::Critical));

        let (state, action) = mode_step(Mode::Auto, None, Some(50.0));
        assert_eq!((written(action), state.over), (255, Override::SensorFailed));
    }

    #[test]
    fn parses_set_mode_requests() {
        let fan = fan();
        let cases = [
            (FanMode::Curve, None, Ok(Mode::Curve)),
            (FanMode::Auto, None, Ok(Mode::Auto)),
            (FanMode::Fixed, Some(60), Ok(Mode::Fixed(60))),
            (FanMode::Fixed, Some(20), Ok(Mode::Fixed(20))),
            (FanMode::Fixed, Some(100), Ok(Mode::Fixed(100))),
            (FanMode::Fixed, None, Err(ChangeError::PercentRequired)),
            (
                FanMode::Fixed,
                Some(19),
                Err(ChangeError::PercentOutOfRange {
                    fan: "case".into(),
                    value: 19,
                    min: 20,
                }),
            ),
            (
                FanMode::Fixed,
                Some(101),
                Err(ChangeError::PercentOutOfRange {
                    fan: "case".into(),
                    value: 101,
                    min: 20,
                }),
            ),
            (
                FanMode::Curve,
                Some(50),
                Err(ChangeError::PercentNotAllowed),
            ),
            (FanMode::Auto, Some(50), Err(ChangeError::PercentNotAllowed)),
        ];
        for (mode, percent, expected) in cases {
            assert_eq!(
                parse_mode(&fan, mode, percent),
                expected,
                "{mode:?} {percent:?}"
            );
        }
    }

    #[test]
    fn restore_mode_never_leaves_manual() {
        let cases = [
            (PwmMode::Automatic(2), PwmMode::Automatic(2)),
            (PwmMode::Automatic(5), PwmMode::Automatic(5)),
            (PwmMode::Manual, AUTOMATIC),
            (PwmMode::Disabled, PwmMode::Disabled),
        ];
        for (original, expected) in cases {
            assert_eq!(restore_mode(original), expected, "{original:?}");
        }
    }
}
