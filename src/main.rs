use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use galaxybook_camera::{
    default_config_path,
    localized_app_name,
    run_smoke_test,
    set_softisp_env,
    setup_singleton,
    APP_ID,
    CameraConfig,
};
use adw::prelude::*;
use libadwaita as adw;

mod ui;

use ui::CameraWindow;

struct CliArgs {
    smoke_test: bool,
    capture_photo_once: Option<PathBuf>,
    config_path: PathBuf,
}

fn parse_args() -> CliArgs {
    let mut smoke_test = false;
    let mut capture_photo_once = None;
    let mut config_path = default_config_path();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke-test" => smoke_test = true,
            "--capture-photo-once" => {
                if let Some(path) = args.next() {
                    capture_photo_once = Some(PathBuf::from(path));
                }
            }
            "--config" => {
                if let Some(path) = args.next() {
                    config_path = PathBuf::from(path);
                }
            }
            _ => {}
        }
    }

    CliArgs {
        smoke_test,
        capture_photo_once,
        config_path,
    }
}

fn main() {
    let args = parse_args();

    if args.smoke_test {
        if let Err(error) = run_smoke_test(&args.config_path) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    let startup_config = CameraConfig::load(&args.config_path);
    if let Some(output_path) = args.capture_photo_once {
        match galaxybook_camera::capture_photo_max_resolution(&startup_config, &output_path) {
            Ok((width, height)) => {
                println!("photo={}", output_path.display());
                println!("resolution={}x{}", width, height);
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }

    set_softisp_env(&startup_config.softisp_mode);

    let singleton = match setup_singleton() {
        Ok(Some(singleton)) => singleton,
        Ok(None) => return,
        Err(error) => {
            eprintln!(
                "Falha ao preparar a instância única do {}: {error}",
                localized_app_name()
            );
            std::process::exit(1);
        }
    };

    let app = adw::Application::builder().application_id(APP_ID).build();
    let config_path = args.config_path.clone();
    let startup_config = Rc::new(startup_config);
    let singleton_holder = Rc::new(RefCell::new(Some(singleton)));

    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let singleton = singleton_holder
            .borrow_mut()
            .take()
            .expect("singleton state should exist on first activation");
        let window = CameraWindow::new(app, config_path.clone(), (*startup_config).clone(), singleton);
        window.start_preview();
        window.present();
    });

    app.run();
}
