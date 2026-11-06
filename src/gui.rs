use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context as _;
use gpui_kit::{
    component::{
        ActiveTheme as _, Disableable as _, IconName, Sizable as _, Theme, ThemeMode, TitleBar,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        form::{field, v_form},
        h_flex,
        input::{Input, InputState},
        select::{Select, SelectEvent, SelectState},
        table::{Column, ColumnSort, DataTable, TableDelegate, TableState},
        v_flex, window_border,
    },
    prelude::FluentBuilder as _,
    *,
};
use human_bytes::human_bytes;
use tokio::runtime::Handle;
use tracing::Level;

use crate::{
    Servers,
    app::dump_results,
    cfg::Cfg,
    logbuf::{LogBuffer, LogLine},
    profiles::{self, Profiles},
    start_servers,
    state::State,
    stop_servers,
};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Initial settings of the proxy, they can be changed in the window before starting it
pub struct Settings {
    /// Settings from the main config file, the default profile
    pub cfg: Cfg,
    pub config_path: PathBuf,
}

pub struct Running {
    pub state: Arc<State>,
    pub servers: Servers,
}

/// The servers started from the GUI, shared with main to stop them after the window is closed
#[derive(Clone, Default)]
pub struct RunningSlot(Arc<Mutex<Option<Running>>>);

impl RunningSlot {
    fn put(&self, running: Running) {
        *self.0.lock().unwrap() = Some(running);
    }

    pub fn take(&self) -> Option<Running> {
        self.0.lock().unwrap().take()
    }
}

/// Runs the GUI on the current thread until the window is closed or `quit` is set
pub fn run(
    rt: Handle,
    settings: Settings,
    running: RunningSlot,
    quit: Arc<AtomicBool>,
    logs: LogBuffer,
) {
    #[cfg(windows)]
    detach_own_console();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            cx.on_window_closed(|cx, _| cx.quit()).detach();

            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("PAProxy".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::centered(size(px(1400.), px(850.)), cx)),
                // Matches paproxy.desktop, Wayland takes the icon from it
                app_id: Some("paproxy".into()),
                #[cfg(target_os = "linux")]
                icon: Some(Arc::new(
                    image::load_from_memory(include_bytes!(
                        "../assets/icons/hicolor/256x256/apps/paproxy.png"
                    ))
                    .expect("Bad window icon")
                    .to_rgba8(),
                )),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| ProxyView::new(rt, settings, running, quit, logs, window, cx))
            })
            .expect("Failed to open window");
            cx.activate(true);
        });
}

/// Closes the console window Windows opens when the exe is started from Explorer,
/// a console shared with a shell is kept
#[cfg(windows)]
fn detach_own_console() {
    use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleProcessList};

    let mut pids = [0u32; 2];
    // SAFETY: the buffer length matches the passed count
    let attached = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
    // Only this process is attached, so the console was created for it
    if attached == 1 {
        // SAFETY: no handles to the console are held at this point
        unsafe { FreeConsole() };
    }
}

struct HandlerRow {
    path: SharedString,
    tx: u64,
    rx: u64,
}

impl HandlerRow {
    fn bytes(&self, col_ix: usize) -> u64 {
        match col_ix {
            1 => self.tx,
            2 => self.rx,
            _ => self.tx + self.rx,
        }
    }
}

struct HandlersDelegate {
    columns: Vec<Column>,
    rows: Vec<HandlerRow>,
    sort: (usize, ColumnSort),
}

impl HandlersDelegate {
    fn new() -> Self {
        Self {
            columns: vec![
                Column::new("handler", "Handler").width(px(560.)).sortable(),
                Column::new("tx", "Tx")
                    .width(px(120.))
                    .text_right()
                    .sortable(),
                Column::new("rx", "Rx")
                    .width(px(120.))
                    .text_right()
                    .sortable(),
                Column::new("sum", "Sum")
                    .width(px(120.))
                    .text_right()
                    .descending(),
            ],
            rows: Vec::new(),
            sort: (3, ColumnSort::Descending),
        }
    }

    fn update(&mut self, state: &State) {
        self.rows = state
            .get_info()
            .iter()
            .map(|r| HandlerRow {
                path: r.key().clone().into(),
                tx: r.0,
                rx: r.1,
            })
            .collect();
        self.apply_sort();
    }

    fn apply_sort(&mut self) {
        match self.sort {
            // Like the TUI: the busiest handlers first
            (_, ColumnSort::Default) => self.rows.sort_by_key(|r| std::cmp::Reverse(r.bytes(3))),
            (0, ColumnSort::Ascending) => self.rows.sort_by(|a, b| a.path.cmp(&b.path)),
            (0, ColumnSort::Descending) => self.rows.sort_by(|a, b| b.path.cmp(&a.path)),
            (col, ColumnSort::Ascending) => self.rows.sort_by_key(|r| r.bytes(col)),
            (col, ColumnSort::Descending) => {
                self.rows.sort_by_key(|r| std::cmp::Reverse(r.bytes(col)))
            }
        }
    }
}

impl TableDelegate for HandlersDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.sort = (col_ix, sort);
        self.apply_sort();
        cx.notify();
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let row = &self.rows[row_ix];
        if col_ix == 0 {
            row.path.clone()
        } else {
            human_bytes(row.bytes(col_ix) as f64).into()
        }
    }
}

/// Editable copy of the proxy settings
struct SettingsForm {
    host: Entity<InputState>,
    port: Entity<InputState>,
    https_port: Entity<InputState>,
    certificate_path: Entity<InputState>,
    key_path: Entity<InputState>,
    sourcedata: Entity<InputState>,
    help: Entity<InputState>,
    upstream_ca_path: Entity<InputState>,
    pagrid: bool,
    transparent: bool,
    log_headers: bool,
    insecure_upstream: bool,
}

impl SettingsForm {
    fn new(cfg: &Cfg, window: &mut Window, cx: &mut App) -> Self {
        let mut input = |value: Option<String>, placeholder: &'static str| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value.unwrap_or_default())
            })
        };

        Self {
            host: input(Some(cfg.host.clone()), "192.168.1.10:5043"),
            port: input(Some(cfg.port.to_string()), "3000"),
            https_port: input(cfg.https_port.map(|p| p.to_string()), "3001"),
            certificate_path: input(cfg.certificate_path.clone(), "Empty to disable HTTPS"),
            key_path: input(cfg.key_path.clone(), "Empty to disable HTTPS"),
            sourcedata: input(cfg.sourcedata.clone(), r"D:\pa\SourceData\www"),
            help: input(cfg.help.clone(), r"D:\pa\SourceData\www\help"),
            upstream_ca_path: input(cfg.upstream_ca_path.clone(), "PEM file, optional"),
            pagrid: cfg.pagrid,
            transparent: cfg.transparent,
            log_headers: cfg.log_headers,
            insecure_upstream: cfg.insecure_upstream,
        }
    }

    /// Replaces the form contents, e.g. when another profile is selected
    fn set(&mut self, cfg: &Cfg, window: &mut Window, cx: &mut App) {
        let mut set = |input: &Entity<InputState>, value: Option<String>| {
            input.update(cx, |state, cx| {
                state.set_value(value.unwrap_or_default(), window, cx)
            });
        };
        set(&self.host, Some(cfg.host.clone()));
        set(&self.port, Some(cfg.port.to_string()));
        set(&self.https_port, cfg.https_port.map(|p| p.to_string()));
        set(&self.certificate_path, cfg.certificate_path.clone());
        set(&self.key_path, cfg.key_path.clone());
        set(&self.sourcedata, cfg.sourcedata.clone());
        set(&self.help, cfg.help.clone());
        set(&self.upstream_ca_path, cfg.upstream_ca_path.clone());
        self.pagrid = cfg.pagrid;
        self.transparent = cfg.transparent;
        self.log_headers = cfg.log_headers;
        self.insecure_upstream = cfg.insecure_upstream;
    }

    fn to_cfg(&self, cx: &App) -> anyhow::Result<Cfg> {
        let text = |input: &Entity<InputState>| -> Option<String> {
            let value = input.read(cx).value().trim().to_owned();
            (!value.is_empty()).then_some(value)
        };
        let port = |input: &Entity<InputState>, name: &str| -> anyhow::Result<Option<u16>> {
            text(input)
                .map(|value| value.parse::<u16>())
                .transpose()
                .with_context(|| format!("{} must be a number from 0 to 65535", name))
        };

        Ok(Cfg {
            port: port(&self.port, "HTTP port")?.context("HTTP port must be set")?,
            https_port: port(&self.https_port, "HTTPS port")?,
            certificate_path: text(&self.certificate_path),
            key_path: text(&self.key_path),
            sourcedata: text(&self.sourcedata),
            help: text(&self.help),
            host: text(&self.host).context("Upstream host must be set")?,
            pagrid: self.pagrid,
            log_headers: self.log_headers,
            transparent: self.transparent,
            insecure_upstream: self.insecure_upstream,
            upstream_ca_path: text(&self.upstream_ca_path),
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Stopped,
    Starting,
    Running,
    Stopping,
}

pub struct ProxyView {
    rt: Handle,
    running: RunningSlot,
    status: Status,
    quit: Arc<AtomicBool>,
    profiles: Profiles,
    /// Name of the selected profile, "Save" writes to it
    profile: String,
    profile_select: Entity<SelectState<Vec<SharedString>>>,
    new_profile_name: Entity<InputState>,
    form: SettingsForm,
    /// Statistics of the current or the last run
    state: Arc<State>,
    logs: LogBuffer,
    logs_total: u64,
    logs_scroll: UniformListScrollHandle,
    follow_logs: bool,
    table: Entity<TableState<HandlersDelegate>>,
    _poll: Task<()>,
}

impl ProxyView {
    fn new(
        rt: Handle,
        settings: Settings,
        running: RunningSlot,
        quit: Arc<AtomicBool>,
        logs: LogBuffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let table = cx.new(|cx| TableState::new(HandlersDelegate::new(), window, cx));

        // Reopen the last used profile, the default one is already loaded by main
        let profiles = Profiles::new(settings.config_path);
        let mut profile = profiles::DEFAULT.to_owned();
        let mut cfg = settings.cfg;
        if let Some(last) = profiles.last().filter(|last| last != profiles::DEFAULT) {
            match profiles.load(&last) {
                Ok(last_cfg) => {
                    cfg = last_cfg;
                    profile = last;
                }
                Err(e) => tracing::error!("{:#}", e),
            }
        }
        let form = SettingsForm::new(&cfg, window, cx);

        let profile_select =
            cx.new(|cx| SelectState::new(profile_items(&profiles), None, window, cx));
        profile_select.update(cx, |select, cx| {
            select.set_selected_value(&profile.clone().into(), window, cx)
        });
        cx.subscribe_in(
            &profile_select,
            window,
            |view, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                let SelectEvent::Confirm(Some(name)) = event else {
                    return;
                };
                view.switch_profile(name.to_string(), window, cx);
            },
        )
        .detach();
        let new_profile_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("New profile name"));

        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this.update(cx, |view, cx| view.poll(cx)).is_err() {
                    break;
                }
            }
        });

        Self {
            rt,
            running,
            status: Status::Stopped,
            quit,
            profiles,
            profile,
            profile_select,
            new_profile_name,
            form,
            state: Arc::new(State::new()),
            logs,
            logs_total: 0,
            logs_scroll: UniformListScrollHandle::new(),
            follow_logs: true,
            table,
            _poll: poll,
        }
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        // Ctrl+C in the console
        if self.quit.load(Ordering::Relaxed) {
            cx.quit();
            return;
        }

        let state = self.state.clone();
        self.table.update(cx, |table, cx| {
            table.delegate_mut().update(&state);
            cx.notify();
        });

        let total = self.logs.total();
        if total != self.logs_total {
            self.logs_total = total;
            if self.follow_logs {
                let last = self.logs.len().saturating_sub(1);
                self.logs_scroll
                    .scroll_to_item(last, ScrollStrategy::Bottom);
            }
        }

        cx.notify();
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let cfg = match self.form.to_cfg(cx) {
            Ok(cfg) => Arc::new(cfg),
            Err(e) => {
                tracing::error!("Invalid settings: {:#}", e);
                return;
            }
        };

        let state = Arc::new(State::new());
        self.state = state.clone();
        self.status = Status::Starting;
        let task = self.rt.spawn(start_servers(cfg, state.clone()));

        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |view, cx| {
                match result {
                    Ok(Ok(servers)) => {
                        view.running.put(Running { state, servers });
                        view.status = Status::Running;
                    }
                    Ok(Err(e)) => {
                        tracing::error!("Failed to start the proxy: {:#}", e);
                        view.status = Status::Stopped;
                    }
                    Err(e) => {
                        tracing::error!("Failed to start the proxy: {}", e);
                        view.status = Status::Stopped;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(running) = self.running.take() else {
            return;
        };

        self.status = Status::Stopping;
        let task = self.rt.spawn(async move {
            stop_servers(running.servers, &running.state).await;
            tracing::info!("Proxy stopped");
        });

        cx.spawn(async move |this, cx| {
            task.await.ok();
            this.update(cx, |view, cx| {
                view.status = Status::Stopped;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Saves the form into the selected profile
    fn save_profile(&self, cx: &mut Context<Self>) {
        let result = self
            .form
            .to_cfg(cx)
            .and_then(|cfg| self.profiles.save(&self.profile, &cfg));
        match result {
            Ok(()) => tracing::info!(
                "Profile {} saved to {}",
                self.profile,
                self.profiles.path(&self.profile).display()
            ),
            Err(e) => tracing::error!("Failed to save profile {}: {:#}", self.profile, e),
        }
    }

    /// Saves the form as a new profile and selects it
    fn save_profile_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.new_profile_name.read(cx).value().trim().to_owned();
        let result = profiles::validate_name(&name)
            .and_then(|_| self.form.to_cfg(cx))
            .and_then(|cfg| self.profiles.save(&name, &cfg));
        if let Err(e) = result {
            tracing::error!("Failed to save profile {}: {:#}", name, e);
            return;
        }

        tracing::info!(
            "Profile {} saved to {}",
            name,
            self.profiles.path(&name).display()
        );
        self.new_profile_name
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.select_profile(name, window, cx);
    }

    fn delete_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.profiles.delete(&self.profile) {
            tracing::error!("Failed to delete profile {}: {:#}", self.profile, e);
            return;
        }
        tracing::info!("Profile {} deleted", self.profile);
        self.switch_profile(profiles::DEFAULT.to_owned(), window, cx);
    }

    /// Loads the profile into the form
    fn switch_profile(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        match self.profiles.load(&name) {
            Ok(cfg) => {
                self.form.set(&cfg, window, cx);
                tracing::info!("Profile {} loaded", name);
            }
            // The default profile may have no file yet, then the form keeps its values
            Err(e) => tracing::error!("{:#}", e),
        }
        self.select_profile(name, window, cx);
    }

    /// Makes the profile current without changing the form
    fn select_profile(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.profiles.set_last(&name);
        let items = profile_items(&self.profiles);
        self.profile_select.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            select.set_selected_value(&name.clone().into(), window, cx);
        });
        self.profile = name;
        cx.notify();
    }

    /// Opens the system dialog and puts the picked file or directory into `input`
    fn browse(
        &self,
        input: Entity<InputState>,
        directories: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: None,
        });

        cx.spawn_in(window, async move |_, cx| {
            let path = match paths.await {
                Ok(Ok(paths)) => paths.and_then(|paths| paths.into_iter().next()),
                Ok(Err(e)) => {
                    tracing::error!("Failed to open the file dialog: {:#}", e);
                    None
                }
                Err(_) => None,
            };
            if let Some(path) = path {
                cx.update(|window, cx| {
                    input.update(cx, |state, cx| {
                        state.set_value(path.display().to_string(), window, cx)
                    })
                })
                .ok();
            }
        })
        .detach();
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let editable = self.status == Status::Stopped;
        let form = &self.form;

        let text_field = |label: &'static str, input: &Entity<InputState>| {
            field()
                .label(label)
                .child(Input::new(input).small().disabled(!editable))
        };
        let path_field = |label: &'static str, input: &Entity<InputState>, directories: bool| {
            let browse = Button::new(SharedString::from(format!("browse-{}", label)))
                .ghost()
                .xsmall()
                .icon(IconName::FolderOpen)
                .tooltip(if directories {
                    "Choose a directory"
                } else {
                    "Choose a file"
                })
                .disabled(!editable)
                .on_click(cx.listener({
                    let input = input.clone();
                    move |view, _, window, cx| view.browse(input.clone(), directories, window, cx)
                }));
            field()
                .label(label)
                .child(Input::new(input).small().disabled(!editable).suffix(browse))
        };
        let check = |id: &'static str,
                     label: &'static str,
                     checked: bool,
                     set: fn(&mut SettingsForm, bool)| {
            field().child(
                Checkbox::new(id)
                    .label(label)
                    .checked(checked)
                    .disabled(!editable)
                    .on_click(cx.listener(move |view, checked: &bool, _, cx| {
                        set(&mut view.form, *checked);
                        cx.notify();
                    })),
            )
        };

        let (status, action) = match self.status {
            Status::Stopped => ("Stopped", "Start"),
            Status::Starting => ("Starting…", "Start"),
            Status::Running => ("Running", "Stop"),
            Status::Stopping => ("Stopping…", "Stop"),
        };
        let status_color = if self.status == Status::Running {
            theme.success
        } else {
            theme.muted_foreground
        };

        v_flex()
            .w(px(360.))
            .flex_none()
            .min_h_0()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child("Settings")
                    .child(div().text_sm().text_color(status_color).child(status)),
            )
            .child(
                v_flex()
                    .p_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Select::new(&self.profile_select)
                                    .small()
                                    .title_prefix("Profile: ")
                                    .disabled(!editable)
                                    .flex_1(),
                            )
                            .child(
                                Button::new("delete-profile")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Delete)
                                    .tooltip("Delete the profile")
                                    .disabled(!editable || self.profile == profiles::DEFAULT)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.delete_profile(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Input::new(&self.new_profile_name)
                                    .small()
                                    .disabled(!editable),
                            )
                            .child(
                                Button::new("save-profile-as")
                                    .small()
                                    .label("Save as")
                                    .tooltip("Save the settings as a new profile")
                                    .disabled(!editable)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.save_profile_as(window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .id("settings")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .child(
                        v_form()
                            .small()
                            .child(text_field("Upstream host", &form.host))
                            .child(check(
                                "pagrid",
                                "PolyAnalyst Grid",
                                form.pagrid,
                                |form, value| form.pagrid = value,
                            ))
                            .child(text_field("HTTP port", &form.port))
                            .child(text_field("HTTPS port", &form.https_port))
                            .child(path_field(
                                "Certificate path",
                                &form.certificate_path,
                                false,
                            ))
                            .child(path_field("Key path", &form.key_path, false))
                            .child(check(
                                "transparent",
                                "Transparent: proxy static files too",
                                form.transparent,
                                |form, value| form.transparent = value,
                            ))
                            .child(path_field("Static files path", &form.sourcedata, true))
                            .child(path_field("Help files path", &form.help, true))
                            .child(check(
                                "insecure-upstream",
                                "Accept any upstream certificate",
                                form.insecure_upstream,
                                |form, value| form.insecure_upstream = value,
                            ))
                            .child(path_field(
                                "Upstream CA path",
                                &form.upstream_ca_path,
                                false,
                            ))
                            .child(check(
                                "log-headers",
                                "Log HTTP headers",
                                form.log_headers,
                                |form, value| form.log_headers = value,
                            )),
                    ),
            )
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .justify_between()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("save-profile")
                            .small()
                            .label("Save")
                            .tooltip("Save the settings into the selected profile")
                            .on_click(cx.listener(|view, _, _, cx| view.save_profile(cx))),
                    )
                    .child(
                        Button::new("start-stop")
                            .small()
                            .primary()
                            .label(action)
                            .disabled(matches!(self.status, Status::Starting | Status::Stopping))
                            .on_click(cx.listener(|view, _, _, cx| match view.status {
                                Status::Stopped => view.start(cx),
                                Status::Running => view.stop(cx),
                                _ => {}
                            })),
                    ),
            )
    }

    fn render_logs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let logs = self.logs.clone();
        let theme = cx.theme();

        v_flex()
            .flex_1()
            .min_h_0()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child("Log")
                    .child(
                        Button::new("follow-logs")
                            .small()
                            .when(self.follow_logs, |button| button.primary())
                            .label("Follow")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.follow_logs = !this.follow_logs;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                uniform_list("logs", logs.len(), move |range, _, cx| {
                    logs.with_lines(range, |line| render_log_line(line, cx))
                })
                .track_scroll(&self.logs_scroll)
                .flex_1()
                .px_2()
                .font_family(theme.mono_font_family.clone())
                .text_sm(),
            )
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .justify_between()
            .items_center()
            .child(format!(
                "HTTP traffic {}    WebSocket traffic {}",
                human_bytes(self.state.total_traffic() as f64),
                human_bytes(self.state.websocket_traffic() as f64)
            ))
            .child(
                Button::new("save-results")
                    .small()
                    .label("Save results")
                    .on_click(cx.listener(|this, _, _, _| {
                        let state = this.state.clone();
                        std::thread::spawn(move || match dump_results(state.get_info()) {
                            Ok(()) => tracing::info!("Results written to logs/results.csv"),
                            Err(e) => tracing::error!("Failed to write results: {}", e),
                        });
                    })),
            )
    }
}

fn render_log_line(line: &LogLine, cx: &App) -> Div {
    let theme = cx.theme();
    let color = match line.level {
        Level::ERROR => theme.danger,
        Level::WARN => theme.warning,
        Level::INFO => theme.foreground,
        _ => theme.muted_foreground,
    };

    div()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .text_color(color)
        .child(format!("{} {:<5} {}", line.time, line.level, line.message))
}

impl Render for ProxyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = v_flex()
            .flex_1()
            .min_h_0()
            .p_2()
            .gap_2()
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .items_stretch()
                    .child(self.render_settings(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(self.render_logs(cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .child(DataTable::new(&self.table).stripe(true)),
                            ),
                    ),
            )
            .child(self.render_status(cx));

        // Some Wayland compositors, e.g. GNOME and WSLg, leave the title bar and the
        // window frame to the application, then they are drawn here
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });

        // No shadow: it takes space around the window and is opaque black without a
        // compositor that supports transparency, as in WSLg
        window_border().shadow_size(px(0.)).child(
            v_flex()
                .size_full()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .when(client_decorated, |this| {
                    this.child(TitleBar::new().child("PAProxy"))
                })
                .child(content),
        )
    }
}

fn profile_items(profiles: &Profiles) -> Vec<SharedString> {
    profiles
        .list()
        .into_iter()
        .map(SharedString::from)
        .collect()
}
