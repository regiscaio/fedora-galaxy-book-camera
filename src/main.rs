use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use galaxybook_camera::{
    configure_libcamera_environment,
    default_config_path,
    init_i18n,
    localized_app_name,
    run_self_test,
    run_smoke_test,
    setup_singleton,
    trf,
    APP_ID,
    CameraConfig,
};
use adw::prelude::*;
use libadwaita as adw;

mod ui;

use ui::CameraWindow;

struct CliArgs {
    self_test: bool,
    smoke_test: bool,
    capture_photo_once: Option<PathBuf>,
    config_path: PathBuf,
}

fn parse_args() -> CliArgs {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from(args: impl IntoIterator<Item = String>) -> CliArgs {
    let mut self_test = false;
    let mut smoke_test = false;
    let mut capture_photo_once = None;
    let mut config_path = default_config_path();

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--self-test" => self_test = true,
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
        self_test,
        smoke_test,
        capture_photo_once,
        config_path,
    }
}

fn main() {
    init_i18n();
    let args = parse_args();
    let startup_config = CameraConfig::load(&args.config_path);
    configure_libcamera_environment(&startup_config.softisp_mode);

    if args.self_test {
        if let Err(error) = run_self_test(&args.config_path) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    if args.smoke_test {
        if let Err(error) = run_smoke_test(&args.config_path) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

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

    let singleton = match setup_singleton() {
        Ok(Some(singleton)) => singleton,
        Ok(None) => return,
        Err(error) => {
            eprintln!("{}", trf(
                "Falha ao preparar a instância única do {app_name}: {error}",
                &[
                    ("app_name", localized_app_name()),
                    ("error", error),
                ],
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_distinct_synthetic_and_hardware_test_modes() {
        let self_test = parse_args_from(["--self-test".to_string()]);
        assert!(self_test.self_test);
        assert!(!self_test.smoke_test);

        let smoke_test = parse_args_from(["--smoke-test".to_string()]);
        assert!(!smoke_test.self_test);
        assert!(smoke_test.smoke_test);
    }
}
