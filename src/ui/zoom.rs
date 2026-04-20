use std::cell::Cell;

use galaxybook_camera::preview_zoom_options;
use gtk::prelude::*;
use gtk::{Align, Orientation};

pub type ZoomSelectorWidgets = (
    gtk::Box,
    gtk::Button,
    gtk::Box,
    gtk::Label,
    Vec<gtk::ToggleButton>,
);

pub fn build_zoom_selector() -> ZoomSelectorWidgets {
    let zoom_strip = gtk::Box::new(Orientation::Horizontal, 6);
    zoom_strip.add_css_class("camera-zoom-strip");
    zoom_strip.set_halign(Align::Center);
    zoom_strip.set_valign(Align::Center);
    zoom_strip.set_visible(false);

    let mut zoom_buttons = Vec::new();
    for option in preview_zoom_options() {
        let button_label = gtk::Label::new(Some(&option.label));
        button_label.add_css_class("camera-zoom-choice-label");
        button_label.set_width_chars(3);
        button_label.set_xalign(0.5);
        button_label.set_yalign(0.5);
        button_label.set_justify(gtk::Justification::Center);
        button_label.set_halign(Align::Center);
        button_label.set_valign(Align::Center);

        let button_center = gtk::CenterBox::new();
        button_center.set_size_request(42, 42);
        button_center.set_halign(Align::Fill);
        button_center.set_valign(Align::Fill);
        button_center.set_center_widget(Some(&button_label));

        let button = gtk::ToggleButton::new();
        button.add_css_class("flat");
        button.add_css_class("camera-zoom-choice");
        button.set_can_focus(false);
        button.set_size_request(42, 42);
        button.set_halign(Align::Center);
        button.set_valign(Align::Center);
        button.set_child(Some(&button_center));
        if let Some(first_button) = zoom_buttons.first() {
            button.set_group(Some(first_button));
        }
        zoom_strip.append(&button);
        zoom_buttons.push(button);
    }

    let zoom_label = gtk::Label::new(Some("1x"));
    zoom_label.add_css_class("camera-zoom-button-label");
    zoom_label.set_width_chars(3);
    zoom_label.set_xalign(0.5);
    zoom_label.set_yalign(0.5);
    zoom_label.set_justify(gtk::Justification::Center);
    zoom_label.set_halign(Align::Center);
    zoom_label.set_valign(Align::Center);

    let zoom_button_center = gtk::CenterBox::new();
    zoom_button_center.set_size_request(42, 42);
    zoom_button_center.set_halign(Align::Fill);
    zoom_button_center.set_valign(Align::Fill);
    zoom_button_center.set_center_widget(Some(&zoom_label));

    let zoom_button = gtk::Button::new();
    zoom_button.set_tooltip_text(Some("Zoom do preview"));
    zoom_button.set_can_focus(false);
    zoom_button.set_size_request(42, 42);
    zoom_button.set_halign(Align::Center);
    zoom_button.set_valign(Align::Center);
    zoom_button.set_child(Some(&zoom_button_center));
    zoom_button.add_css_class("flat");
    zoom_button.add_css_class("camera-mode-button");
    zoom_button.add_css_class("camera-zoom-button");
    zoom_button.set_overflow(gtk::Overflow::Hidden);

    let zoom_root = gtk::Box::new(Orientation::Horizontal, 0);
    zoom_root.set_halign(Align::Center);
    zoom_root.set_valign(Align::Center);
    zoom_root.append(&zoom_button);
    zoom_root.append(&zoom_strip);

    (zoom_root, zoom_button, zoom_strip, zoom_label, zoom_buttons)
}

pub fn refresh_zoom_selector(
    selected_index: usize,
    zoom_button: &gtk::Button,
    zoom_label: &gtk::Label,
    zoom_option_buttons: &[gtk::ToggleButton],
    syncing_ui: &Cell<bool>,
) {
    let selected_option = preview_zoom_options()
        .get(selected_index)
        .or_else(|| preview_zoom_options().first());
    let selected_label = selected_option.map(|option| option.label.as_str()).unwrap_or("1x");

    zoom_label.set_label(selected_label);
    zoom_button.set_tooltip_text(Some(&format!("Zoom do preview: {selected_label}")));

    let was_syncing = syncing_ui.replace(true);
    for (index, button) in zoom_option_buttons.iter().enumerate() {
        let is_active = index == selected_index;
        button.set_active(is_active);
        if is_active {
            button.add_css_class("camera-zoom-choice-active");
        } else {
            button.remove_css_class("camera-zoom-choice-active");
        }
    }
    syncing_ui.set(was_syncing);
}

pub fn set_zoom_selector_expanded(
    zoom_button: &gtk::Button,
    zoom_strip: &gtk::Box,
    expanded: bool,
) {
    zoom_button.set_visible(!expanded);
    zoom_strip.set_visible(expanded);
}
