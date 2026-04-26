use std::sync::mpsc;
use std::time::Duration;

use galaxybook_camera::{install_package_updates, package_update_names, tr, trf};
use gtk::glib;
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use super::CameraWindow;

const CAMERA_UPDATE_PACKAGES: &[&str] = &[
    "galaxybook-camera",
    "galaxybook-ov02c10-kmod-common",
    "akmod-galaxybook-ov02c10",
];

fn update_button_tooltip(packages: &[String]) -> String {
    trf(
        "Baixar e instalar atualizações: {packages}",
        &[("packages", packages.join(", "))],
    )
}

impl CameraWindow {
    pub(super) fn refresh_updates(&self) {
        self.update_button.set_visible(false);
        self.update_button.set_sensitive(false);

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(package_update_names(CAMERA_UPDATE_PACKAGES));
        });

        let update_button = self.update_button.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            match receiver.try_recv() {
                Ok(Ok(packages)) => {
                    let has_updates = !packages.is_empty();
                    update_button.set_visible(has_updates);
                    update_button.set_sensitive(has_updates);
                    if has_updates {
                        update_button.set_tooltip_text(Some(&update_button_tooltip(&packages)));
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(_error)) => {
                    update_button.set_visible(false);
                    update_button.set_sensitive(false);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    update_button.set_visible(false);
                    update_button.set_sensitive(false);
                    glib::ControlFlow::Break
                }
            }
        });
    }

    pub(super) fn install_updates(&self) {
        if !self.update_button.is_visible() || !self.update_button.is_sensitive() {
            return;
        }

        self.update_button.set_sensitive(false);
        self.toast_overlay.add_toast(adw::Toast::new(&tr(
            "Baixando e instalando atualizações do Galaxy Book Câmera…",
        )));

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(install_package_updates(CAMERA_UPDATE_PACKAGES));
        });

        let update_button = self.update_button.clone();
        let toast_overlay = self.toast_overlay.clone();
        let window = self.window.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            match receiver.try_recv() {
                Ok(Ok(_output)) => {
                    update_button.set_visible(false);
                    toast_overlay.add_toast(adw::Toast::new(&tr(
                        "Atualizações instaladas. Reinicie o app se ele tiver sido atualizado.",
                    )));
                    glib::ControlFlow::Break
                }
                Ok(Err(error)) => {
                    update_button.set_sensitive(true);
                    present_update_result_dialog(&window, &tr("Atualizar pacotes"), &error);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    update_button.set_sensitive(true);
                    toast_overlay.add_toast(adw::Toast::new(&tr(
                        "Falha ao acompanhar a atualização solicitada.",
                    )));
                    glib::ControlFlow::Break
                }
            }
        });
    }
}

fn present_update_result_dialog(parent: &adw::ApplicationWindow, title: &str, output: &str) {
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(680)
        .content_height(420)
        .build();

    let header = adw::HeaderBar::new();
    let window_title = adw::WindowTitle::new(title, &tr("Saída da atualização"));
    header.set_title_widget(Some(&window_title));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    let text_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(16)
        .bottom_margin(16)
        .left_margin(16)
        .right_margin(16)
        .build();
    let fallback_output = tr("A atualização falhou, mas não retornou saída textual.");
    let output_text = if output.trim().is_empty() {
        fallback_output.as_str()
    } else {
        output
    };
    text_view.buffer().set_text(output_text);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&text_view)
        .build();

    toolbar.set_content(Some(&scroller));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}
