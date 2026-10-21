// Project: miner-widget-stats
// File: src/main.rs
// Description: macOS menu bar widget that shows the mining pool hash rate and the room temperature.
// Author: Jan Alexandr Kopřiva
// License: MIT

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::thread;

use miner_widget_stats::config::{self, Config};
use miner_widget_stats::logger;
use miner_widget_stats::widget::{open_log, run_exclusive, update_once, INITIAL_TITLE};
use reqwest::blocking::Client;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

enum UserEvent {
    Title(String),
    Menu(MenuEvent),
}

/// Refresh once and hand the title to the event loop, which owns the menu bar.
/// `wait` false skips the refresh when another one is running.
fn refresh(
    client: &Client,
    config: &Config,
    proxy: &EventLoopProxy<UserEvent>,
    gate: &Mutex<()>,
    wait: bool,
) -> bool {
    let title = run_exclusive(gate, wait, || update_once(client, config));
    let ran = title.is_some();
    if let Some(title) = title {
        let _ = proxy.send_event(UserEvent::Title(title));
    }
    ran
}

fn main() {
    let (root, env_file) = config::locate();
    let logs_dir = root.join("logs");
    logger::init(&logs_dir);
    if let Some(path) = &env_file {
        // Like load_dotenv() in Python, a variable already in the environment wins.
        for problem in config::load_dotenv(path, |key, value| {
            if std::env::var_os(key).is_none() {
                std::env::set_var(key, value);
            }
        }) {
            logger::error(&problem);
        }
    }
    let config = match Config::from_env() {
        Ok(config) => Arc::new(config),
        Err(message) => {
            logger::error(&message);
            std::process::exit(2);
        }
    };
    let client = Client::new();
    let gate = Arc::new(Mutex::new(()));

    // Accessory keeps the Dock icon away, as LSUIElement did for the Python version.
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }
    }));

    let refresh_item = MenuItem::new("Aktualizovat", true, None);
    let log_item = MenuItem::new("Otevřít log", true, None);
    let quit_item = MenuItem::new("Ukončit", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &refresh_item,
        &log_item,
        &quit_item,
        &PredefinedMenuItem::separator(),
    ])
    .expect("menu items");
    let (refresh_id, log_id, quit_id) = (
        refresh_item.id().clone(),
        log_item.id().clone(),
        quit_item.id().clone(),
    );

    // Refresh once now, then every UPDATE_INTERVAL seconds. A panic must not end the loop.
    {
        let (client, config, proxy, gate) =
            (client.clone(), config.clone(), proxy.clone(), gate.clone());
        logger::info("Widget initialized");
        logger::info("Starting the refresh loop");
        thread::spawn(move || loop {
            match catch_unwind(AssertUnwindSafe(|| {
                refresh(&client, &config, &proxy, &gate, true)
            })) {
                Ok(_) => thread::sleep(config.update_interval),
                Err(_) => {
                    logger::error("Error in the refresh thread");
                    thread::sleep(config.retry_delay);
                }
            }
        });
    }

    let mut tray = None;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            // The tray must be created once the loop is running.
            Event::NewEvents(StartCause::Init) => {
                logger::info("Starting the application");
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu.clone()))
                        .with_title(INITIAL_TITLE)
                        .build()
                        .expect("menu bar item"),
                );
            }
            Event::UserEvent(UserEvent::Title(title)) => {
                if let Some(tray) = &tray {
                    tray.set_title(Some(title));
                }
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                if event.id == refresh_id {
                    // Off the event loop, so a slow pool does not freeze the menu.
                    let (client, config, proxy, gate) =
                        (client.clone(), config.clone(), proxy.clone(), gate.clone());
                    thread::spawn(move || {
                        logger::info("Manual refresh started");
                        if refresh(&client, &config, &proxy, &gate, false) {
                            logger::info("Manual refresh finished");
                        } else {
                            logger::info("A refresh is already running, skipping");
                        }
                    });
                } else if event.id == log_id {
                    open_log(&logs_dir.join("miner.log"));
                } else if event.id == quit_id {
                    logger::info("Shutting down");
                    tray = None;
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}
