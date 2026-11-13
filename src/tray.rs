#[cfg(any(target_os = "windows", target_os = "macos", feature = "tray"))]
mod enabled {
    use crate::db::Database;
    use crate::scheduler::SchedulerHandle;
    use anyhow::{Context, Result};
    #[cfg(not(target_os = "windows"))]
    use std::process::Command;
    use tokio::sync::oneshot;
    use tracing::{error, info, warn};
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
    use winit::application::ApplicationHandler;
    use winit::event::StartCause;
    use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
    use winit::window::WindowId;

    const ICON_PNG: &[u8] = include_bytes!("web/assets/icon.png");

    #[derive(Debug)]
    enum UserEvent {
        Menu(MenuEvent),
        Tray(TrayIconEvent),
    }

    struct TrayApp {
        tray_icon: Option<TrayIcon>,
        proxy: EventLoopProxy<UserEvent>,
        webui_url: String,
        db: Database,
        scheduler: SchedulerHandle,
        rt_handle: tokio::runtime::Handle,
        shutdown_tx: Option<oneshot::Sender<()>>,
        open_id: MenuId,
        check_all_id: MenuId,
        quit_id: MenuId,
    }

    impl TrayApp {
        fn trigger_check_all(&self) {
            let db = self.db.clone();
            let scheduler = self.scheduler.clone();
            self.rt_handle.spawn(async move {
                match db.list_trackers() {
                    Ok(trackers) => {
                        let enabled: Vec<_> = trackers.into_iter().filter(|t| t.enabled).collect();
                        info!(count = enabled.len(), "Tray requested check for all enabled trackers");
                        for t in enabled {
                            scheduler.run_now(t.id).await;
                        }
                    }
                    Err(e) => error!("Failed to list trackers from tray: {e}"),
                }
            });
        }

        fn build_tray(&mut self) -> Result<()> {
            let proxy_menu = self.proxy.clone();
            MenuEvent::set_event_handler(Some(move |event| {
                let _ = proxy_menu.send_event(UserEvent::Menu(event));
            }));

            let proxy_tray = self.proxy.clone();
            TrayIconEvent::set_event_handler(Some(move |event| {
                let _ = proxy_tray.send_event(UserEvent::Tray(event));
            }));

            let menu = Menu::new();
            let status_item = MenuItem::new(format!("PTNotifier • {}", self.webui_url), false, None);
            let open_item = MenuItem::new("Open WebUI", true, None);
            let check_all_item = MenuItem::new("Check All Trackers Now", true, None);
            let quit_item = MenuItem::new("Quit", true, None);

            self.open_id = open_item.id().clone();
            self.check_all_id = check_all_item.id().clone();
            self.quit_id = quit_item.id().clone();

            menu.append(&status_item)?;
            menu.append(&PredefinedMenuItem::separator())?;
            menu.append(&open_item)?;
            menu.append(&check_all_item)?;
            menu.append(&PredefinedMenuItem::separator())?;
            menu.append(&quit_item)?;

            let icon = load_tray_icon()?;
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip(format!("PTNotifier ({})", self.webui_url))
                .with_icon(icon)
                .build()
                .context("Failed to create system tray icon")?;

            self.tray_icon = Some(tray);
            info!(url = %self.webui_url, "System tray icon active");
            Ok(())
        }
    }

    impl ApplicationHandler<UserEvent> for TrayApp {
        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            _event: winit::event::WindowEvent,
        ) {
        }

        fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
            if cause == StartCause::Init
                && self.tray_icon.is_none()
                && let Err(e) = self.build_tray()
            {
                error!("Failed to initialize system tray: {e}");
                if let Some(tx) = self.shutdown_tx.take() {
                    let _ = tx.send(());
                }
                event_loop.exit();
            }
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
            match event {
                UserEvent::Menu(menu_ev) => {
                    if menu_ev.id == self.open_id {
                        open_browser(&self.webui_url);
                    } else if menu_ev.id == self.check_all_id {
                        self.trigger_check_all();
                    } else if menu_ev.id == self.quit_id {
                        info!("Quit selected from system tray");
                        if let Some(tx) = self.shutdown_tx.take() {
                            let _ = tx.send(());
                        }
                        self.tray_icon.take();
                        event_loop.exit();
                    }
                }
                UserEvent::Tray(tray_ev) => {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = tray_ev
                    {
                        open_browser(&self.webui_url);
                    }
                }
            }
        }
    }

    fn load_tray_icon() -> Result<Icon> {
        let img = image::load_from_memory(ICON_PNG)
            .context("Failed to decode embedded icon.png")?
            .into_rgba8();
        let (width, height) = img.dimensions();
        Icon::from_rgba(img.into_raw(), width, height).context("Failed to create tray Icon")
    }

    fn open_browser(url: &str) {
        #[cfg(target_os = "windows")]
        {
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            #[link(name = "shell32")]
            unsafe extern "system" {
                fn ShellExecuteW(
                    hwnd: *mut core::ffi::c_void,
                    lp_operation: *const u16,
                    lp_file: *const u16,
                    lp_parameters: *const u16,
                    lp_directory: *const u16,
                    n_show_cmd: i32,
                ) -> isize;
            }
            const SW_SHOWNORMAL: i32 = 1;
            let op: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
            let file: Vec<u16> = OsStr::new(url).encode_wide().chain(Some(0)).collect();
            let code = unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    op.as_ptr(),
                    file.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
            if code <= 32 {
                warn!(url = %url, code, "Failed to open browser via ShellExecuteW");
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            #[cfg(target_os = "macos")]
            let res = Command::new("open").arg(url).spawn();

            #[cfg(all(unix, not(target_os = "macos")))]
            let res = Command::new("xdg-open").arg(url).spawn();

            if let Err(e) = res {
                warn!(url = %url, "Failed to open browser: {e}");
            }
        }
    }

    /// If launched in a dedicated console window on Windows (e.g. double-clicking or via shortcut),
    /// detach from it so only the tray icon remains visible. Keep console if run from an existing shell.
    #[cfg(target_os = "windows")]
    fn maybe_hide_console() {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetConsoleProcessList(process_list: *mut u32, process_count: u32) -> u32;
            fn FreeConsole() -> i32;
        }
        unsafe {
            let mut pids = [0u32; 2];
            let count = GetConsoleProcessList(pids.as_mut_ptr(), 2);
            if count == 1 {
                let _ = FreeConsole();
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn maybe_hide_console() {}

    pub fn run_tray(
        webui_url: String,
        db: Database,
        scheduler: SchedulerHandle,
        rt_handle: tokio::runtime::Handle,
        shutdown_tx: oneshot::Sender<()>,
    ) -> Result<()> {
        maybe_hide_console();

        let mut builder = EventLoop::<UserEvent>::with_user_event();
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }
        let event_loop = builder.build().context("Failed to create tray event loop")?;
        let proxy = event_loop.create_proxy();

        let mut app = TrayApp {
            tray_icon: None,
            proxy,
            webui_url,
            db,
            scheduler,
            rt_handle,
            shutdown_tx: Some(shutdown_tx),
            open_id: MenuId::new(""),
            check_all_id: MenuId::new(""),
            quit_id: MenuId::new(""),
        };

        event_loop.run_app(&mut app).context("Tray event loop error")?;
        Ok(())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", feature = "tray"))]
pub use enabled::run_tray;

#[cfg(not(any(target_os = "windows", target_os = "macos", feature = "tray")))]
pub fn run_tray(
    _webui_url: String,
    _db: crate::db::Database,
    _scheduler: crate::scheduler::SchedulerHandle,
    _rt_handle: tokio::runtime::Handle,
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
) -> anyhow::Result<()> {
    anyhow::bail!(
        "System tray support is not compiled into this Linux build. Rebuild with `cargo build --features tray` (requires libgtk-3 and libayatana-appindicator3)."
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_png_icon_decodes_as_rgba() {
        let bytes = include_bytes!("web/assets/icon.png");
        let img = image::load_from_memory(bytes).expect("valid icon.png");
        let rgba = img.into_rgba8();
        assert_eq!(rgba.dimensions(), (64, 64));
    }
}
