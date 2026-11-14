#[cfg(any(target_os = "windows", target_os = "macos", feature = "tray"))]
mod enabled {
    use crate::db::Database;
    use crate::scheduler::SchedulerHandle;
    use anyhow::{Context, Result};
    #[cfg(not(target_os = "windows"))]
    use std::process::Command;
    use tracing::{error, info, warn};
    use tray_icon::Icon;
    use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};

    const ICON_PNG: &[u8] = include_bytes!("web/assets/icon.png");

    struct MenuIds {
        open: MenuId,
        check_all: MenuId,
        quit: MenuId,
    }

    fn build_menu(webui_url: &str) -> Result<(Menu, MenuIds)> {
        let menu = Menu::new();
        let status_item = MenuItem::new(format!("PTNotifier • {webui_url}"), false, None);
        let open_item = MenuItem::new("Open WebUI", true, None);
        let check_all_item = MenuItem::new("Check All Trackers Now", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        menu.append(&status_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&open_item)?;
        menu.append(&check_all_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit_item)?;

        let ids = MenuIds {
            open: open_item.id().clone(),
            check_all: check_all_item.id().clone(),
            quit: quit_item.id().clone(),
        };
        Ok((menu, ids))
    }

    fn trigger_check_all(
        db: &Database,
        scheduler: &SchedulerHandle,
        rt_handle: &tokio::runtime::Handle,
    ) {
        let db = db.clone();
        let scheduler = scheduler.clone();
        rt_handle.spawn(async move {
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

    /// winit-driven tray for Windows and macOS.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    mod event_loop {
        use super::*;
        use tokio::sync::oneshot;
        use tray_icon::menu::MenuEvent;
        use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
        use winit::application::ApplicationHandler;
        use winit::event::StartCause;
        use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
        use winit::window::WindowId;

        #[derive(Debug)]
        enum UserEvent {
            Menu(MenuEvent),
            Tray(TrayIconEvent),
            /// The process is shutting down (e.g. Ctrl+C): leave the event loop.
            Shutdown,
        }

        struct TrayApp {
            tray_icon: Option<TrayIcon>,
            proxy: EventLoopProxy<UserEvent>,
            webui_url: String,
            db: Database,
            scheduler: SchedulerHandle,
            rt_handle: tokio::runtime::Handle,
            shutdown_tx: Option<oneshot::Sender<()>>,
            ids: Option<MenuIds>,
        }

        impl TrayApp {
            fn build_tray(&mut self) -> Result<()> {
                let proxy_menu = self.proxy.clone();
                MenuEvent::set_event_handler(Some(move |event| {
                    let _ = proxy_menu.send_event(UserEvent::Menu(event));
                }));

                let proxy_tray = self.proxy.clone();
                TrayIconEvent::set_event_handler(Some(move |event| {
                    let _ = proxy_tray.send_event(UserEvent::Tray(event));
                }));

                let (menu, ids) = build_menu(&self.webui_url)?;
                let icon = load_tray_icon()?;
                let tray = TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_tooltip(format!("PTNotifier ({})", self.webui_url))
                    .with_icon(icon)
                    .build()
                    .context("Failed to create system tray icon")?;

                self.ids = Some(ids);
                self.tray_icon = Some(tray);
                info!(url = %self.webui_url, "System tray icon active");
                Ok(())
            }

            fn quit(&mut self, event_loop: &ActiveEventLoop) {
                if let Some(tx) = self.shutdown_tx.take() {
                    let _ = tx.send(());
                }
                self.tray_icon.take();
                event_loop.exit();
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
                    self.quit(event_loop);
                }
            }

            fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
                match event {
                    UserEvent::Menu(menu_ev) => {
                        let Some(ids) = &self.ids else { return };
                        if menu_ev.id == ids.open {
                            open_browser(&self.webui_url);
                        } else if menu_ev.id == ids.check_all {
                            trigger_check_all(&self.db, &self.scheduler, &self.rt_handle);
                        } else if menu_ev.id == ids.quit {
                            info!("Quit selected from system tray");
                            self.quit(event_loop);
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
                    UserEvent::Shutdown => self.quit(event_loop),
                }
            }
        }

        pub fn run_tray(
            webui_url: String,
            db: Database,
            scheduler: SchedulerHandle,
            rt_handle: tokio::runtime::Handle,
            shutdown_tx: oneshot::Sender<()>,
            exit_rx: oneshot::Receiver<()>,
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

            // Process shutdown (Ctrl+C) observed by the server must also end the tray loop.
            let exit_proxy = proxy.clone();
            rt_handle.spawn(async move {
                if exit_rx.await.is_ok() {
                    let _ = exit_proxy.send_event(UserEvent::Shutdown);
                }
            });

            let mut app = TrayApp {
                tray_icon: None,
                proxy,
                webui_url,
                db,
                scheduler,
                rt_handle,
                shutdown_tx: Some(shutdown_tx),
                ids: None,
            };

            event_loop.run_app(&mut app).context("Tray event loop error")?;
            Ok(())
        }
    }

    /// GTK-driven tray for Linux/BSD: `tray-icon` needs a GTK main loop running on the thread
    /// that creates the icon, which winit does not provide.
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    mod event_loop {
        use super::*;
        use gtk::glib;
        use std::sync::{Arc, Mutex};
        use tokio::sync::oneshot;
        use tray_icon::TrayIconBuilder;
        use tray_icon::menu::MenuEvent;

        /// Thread-safe: queues `gtk::main_quit` onto the GTK main context.
        fn quit_main_loop() {
            glib::MainContext::default().invoke(gtk::main_quit);
        }

        pub fn run_tray(
            webui_url: String,
            db: Database,
            scheduler: SchedulerHandle,
            rt_handle: tokio::runtime::Handle,
            shutdown_tx: oneshot::Sender<()>,
            exit_rx: oneshot::Receiver<()>,
        ) -> Result<()> {
            if let Err(e) = gtk::init() {
                let _ = shutdown_tx.send(());
                anyhow::bail!("Failed to initialize GTK for the system tray: {e}");
            }

            let built = build_menu(&webui_url).and_then(|(menu, ids)| {
                let icon = load_tray_icon()?;
                let tray = TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_tooltip(format!("PTNotifier ({webui_url})"))
                    .with_icon(icon)
                    .build()
                    .context("Failed to create system tray icon")?;
                Ok((tray, ids))
            });
            let (tray, ids) = match built {
                Ok(v) => v,
                Err(e) => {
                    error!("Failed to initialize system tray: {e}");
                    let _ = shutdown_tx.send(());
                    return Err(e);
                }
            };
            info!(url = %webui_url, "System tray icon active");

            let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));
            let handler_shutdown = shutdown_tx.clone();
            let handler_rt = rt_handle.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == ids.open {
                    open_browser(&webui_url);
                } else if event.id == ids.check_all {
                    trigger_check_all(&db, &scheduler, &handler_rt);
                } else if event.id == ids.quit {
                    info!("Quit selected from system tray");
                    if let Some(tx) = handler_shutdown.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    quit_main_loop();
                }
            }));

            // Process shutdown (Ctrl+C) observed by the server must also end the GTK loop.
            rt_handle.spawn(async move {
                if exit_rx.await.is_ok() {
                    quit_main_loop();
                }
            });

            gtk::main();

            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
            drop(tray);
            if let Some(tx) = shutdown_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            Ok(())
        }
    }

    pub use event_loop::run_tray;

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

            match res {
                // Reap the launcher off the tray thread: a dropped Child is never waited on
                // and would stay a zombie until PTNotifier exits.
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(e) => warn!(url = %url, "Failed to open browser: {e}"),
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

    #[cfg(target_os = "macos")]
    fn maybe_hide_console() {}
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
    _exit_rx: tokio::sync::oneshot::Receiver<()>,
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
