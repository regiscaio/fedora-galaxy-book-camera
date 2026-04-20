use std::rc::Rc;

use adw::prelude::*;
use galaxybook_camera::{
    default_config_path,
    photo_library_dir,
    preview_zoom_options,
    video_library_dir,
    APP_ID,
    APP_NAME,
    AudioSourceOption,
    Preset,
    VideoEncoderBackend,
};
use gtk::glib;
use gtk::prelude::*;
use gtk::{Align, Orientation};
use libadwaita as adw;

pub struct ControlWidgets {
    pub auto_apply_row: adw::SwitchRow,
    pub show_grid_row: adw::SwitchRow,
    pub mirror_row: adw::SwitchRow,
    pub record_audio_row: adw::SwitchRow,
    pub preset_row: adw::ComboRow,
    pub audio_source_row: adw::ComboRow,
    pub brightness_scale: gtk::Scale,
    pub brightness_value: gtk::Label,
    pub exposure_scale: gtk::Scale,
    pub exposure_value: gtk::Label,
    pub contrast_scale: gtk::Scale,
    pub contrast_value: gtk::Label,
    pub saturation_scale: gtk::Scale,
    pub saturation_value: gtk::Label,
    pub hue_scale: gtk::Scale,
    pub hue_value: gtk::Label,
    pub temperature_scale: gtk::Scale,
    pub temperature_value: gtk::Label,
    pub tint_scale: gtk::Scale,
    pub tint_value: gtk::Label,
    pub red_scale: gtk::Scale,
    pub red_value: gtk::Label,
    pub green_scale: gtk::Scale,
    pub green_value: gtk::Label,
    pub blue_scale: gtk::Scale,
    pub blue_value: gtk::Label,
    pub gamma_scale: gtk::Scale,
    pub gamma_value: gtk::Label,
    pub sharpness_scale: gtk::Scale,
    pub sharpness_value: gtk::Label,
    pub apply_button: gtk::Button,
    pub save_button: gtk::Button,
    pub reset_button: gtk::Button,
}

pub fn build_suffix_action_row<F>(
    title: &str,
    subtitle: &str,
    icon_name: &str,
    tooltip: &str,
    on_activate: F,
) -> adw::ActionRow
where
    F: Fn() + 'static,
{
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    row.set_subtitle_selectable(true);

    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tooltip)
        .valign(Align::Center)
        .build();
    button.add_css_class("flat");

    let callback = Rc::new(on_activate);
    {
        let callback = callback.clone();
        button.connect_clicked(move |_| {
            callback();
        });
    }

    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    row.set_activatable(true);

    row
}

pub fn build_about_summary_row(app_name: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);

    let content = gtk::Box::new(Orientation::Horizontal, 16);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let app_icon = gtk::Image::from_icon_name(APP_ID);
    app_icon.set_pixel_size(48);
    app_icon.set_valign(Align::Start);

    let text_column = gtk::Box::new(Orientation::Vertical, 4);
    text_column.set_hexpand(true);
    text_column.set_valign(Align::Center);

    let title_row = gtk::Box::new(Orientation::Horizontal, 10);
    title_row.set_halign(Align::Start);

    let title_label = gtk::Label::new(None);
    title_label.set_markup(&format!(
        "<span size='large' weight='600'>{}</span>",
        glib::markup_escape_text(app_name)
    ));
    title_label.set_xalign(0.0);

    let version_label = gtk::Label::new(None);
    version_label.set_markup(&format!(
        "<span alpha='55%' size='small'>Versão {}</span>",
        glib::markup_escape_text(env!("CARGO_PKG_VERSION"))
    ));
    version_label.set_xalign(0.0);

    title_row.append(&title_label);
    title_row.append(&version_label);

    let description_label = gtk::Label::new(None);
    description_label.set_markup(
        "<span alpha='55%' size='small'>Aplicativo de câmera nativo para Fedora no Galaxy Book.</span>",
    );
    description_label.set_xalign(0.0);
    description_label.set_wrap(true);

    text_column.append(&title_row);
    text_column.append(&description_label);

    content.append(&app_icon);
    content.append(&text_column);
    row.set_child(Some(&content));

    row
}

pub fn build_about_details_subpage() -> adw::NavigationPage {
    let page = adw::PreferencesPage::builder()
        .name("details")
        .title("Detalhes")
        .build();

    let app_group = adw::PreferencesGroup::builder()
        .title("Aplicativo")
        .description("Identificação pública e técnica do Galaxy Book Câmera.")
        .build();

    for (title, subtitle) in [
        ("Nome", APP_NAME.to_string()),
        ("Versão", env!("CARGO_PKG_VERSION").to_string()),
        ("App ID", APP_ID.to_string()),
        ("Desktop ID", format!("{APP_ID}.desktop")),
    ] {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .build();
        row.set_activatable(false);
        row.set_subtitle_selectable(true);
        app_group.add(&row);
    }

    let storage_group = adw::PreferencesGroup::builder()
        .title("Armazenamento")
        .description("Arquivos locais e diretórios usados pelo aplicativo.")
        .build();

    for (title, subtitle) in [
        ("Configuração", default_config_path().display().to_string()),
        ("Fotos", photo_library_dir().display().to_string()),
        ("Vídeos", video_library_dir().display().to_string()),
    ] {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .build();
        row.set_activatable(false);
        row.set_subtitle_selectable(true);
        storage_group.add(&row);
    }

    page.add(&app_group);
    page.add(&storage_group);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(0)
        .child(&page)
        .build();

    adw::NavigationPage::builder()
        .title("Detalhes")
        .tag("details")
        .child(&scroller)
        .can_pop(true)
        .build()
}

pub fn build_zoom_selector() -> (
    gtk::Box,
    gtk::Button,
    gtk::Box,
    gtk::Label,
    Vec<gtk::ToggleButton>,
) {
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

pub fn build_control_widgets(audio_sources: &[AudioSourceOption]) -> ControlWidgets {
    let auto_apply_row = adw::SwitchRow::builder()
        .title("Aplicar automaticamente")
        .subtitle("Envie os ajustes assim que eles mudarem.")
        .build();
    let show_grid_row = adw::SwitchRow::builder()
        .title("Guia de Composição")
        .subtitle("Mostra a regra dos terços sobre o preview.")
        .build();
    let mirror_row = adw::SwitchRow::builder()
        .title("Espelhar imagem")
        .subtitle("Vale para preview, foto e vídeo.")
        .build();
    let record_audio_row = adw::SwitchRow::builder()
        .title("Gravar áudio")
        .subtitle("Usa o microfone padrão ou a fonte selecionada.")
        .build();

    let preset_row = build_combo_row(
        "Preset",
        None,
        &Preset::all()
            .iter()
            .map(|preset| preset.label().to_string())
            .collect::<Vec<_>>(),
    );
    let audio_source_row = build_combo_row(
        "Fonte de áudio",
        None,
        &audio_sources
            .iter()
            .map(|source| source.label.clone())
            .collect::<Vec<_>>(),
    );

    let (brightness_scale, brightness_value) = build_scale(-0.20, 0.25, 0.01);
    let (exposure_scale, exposure_value) = build_scale(-0.50, 1.00, 0.05);
    let (contrast_scale, contrast_value) = build_scale(0.50, 2.00, 0.01);
    let (saturation_scale, saturation_value) = build_scale(0.00, 2.20, 0.01);
    let (hue_scale, hue_value) = build_scale(-1.00, 1.00, 0.01);
    let (temperature_scale, temperature_value) = build_scale(-1.00, 1.00, 0.01);
    let (tint_scale, tint_value) = build_scale(-1.00, 1.00, 0.01);
    let (red_scale, red_value) = build_scale(0.50, 1.50, 0.01);
    let (green_scale, green_value) = build_scale(0.50, 1.50, 0.01);
    let (blue_scale, blue_value) = build_scale(0.50, 1.50, 0.01);
    let (gamma_scale, gamma_value) = build_scale(0.50, 1.80, 0.01);
    let (sharpness_scale, sharpness_value) = build_scale(1.00, 2.00, 0.01);

    let apply_button = gtk::Button::with_label("Aplicar");
    apply_button.add_css_class("suggested-action");
    let save_button = gtk::Button::with_label("Salvar");
    let reset_button = gtk::Button::with_label("Resetar");

    ControlWidgets {
        auto_apply_row,
        show_grid_row,
        mirror_row,
        record_audio_row,
        preset_row,
        audio_source_row,
        brightness_scale,
        brightness_value,
        exposure_scale,
        exposure_value,
        contrast_scale,
        contrast_value,
        saturation_scale,
        saturation_value,
        hue_scale,
        hue_value,
        temperature_scale,
        temperature_value,
        tint_scale,
        tint_value,
        red_scale,
        red_value,
        green_scale,
        green_value,
        blue_scale,
        blue_value,
        gamma_scale,
        gamma_value,
        sharpness_scale,
        sharpness_value,
        apply_button,
        save_button,
        reset_button,
    }
}

pub fn build_sidebar(
    controls: &ControlWidgets,
    encoder_backend: VideoEncoderBackend,
) -> gtk::ScrolledWindow {
    let page = adw::PreferencesPage::new();

    let flow_group = adw::PreferencesGroup::builder()
        .title("Fluxo")
        .description("Comportamento do preview e ações rápidas.")
        .build();
    flow_group.add(&controls.auto_apply_row);
    flow_group.add(&controls.show_grid_row);
    flow_group.add(&controls.mirror_row);

    let action_box = gtk::Box::new(Orientation::Horizontal, 0);
    action_box.add_css_class("linked");
    action_box.set_hexpand(true);
    action_box.set_halign(Align::Fill);
    action_box.set_homogeneous(true);
    controls.apply_button.set_hexpand(true);
    controls.save_button.set_hexpand(true);
    controls.reset_button.set_hexpand(true);
    action_box.append(&controls.apply_button);
    action_box.append(&controls.save_button);
    action_box.append(&controls.reset_button);
    let action_row = gtk::ListBoxRow::new();
    action_row.set_activatable(false);
    action_row.set_selectable(false);
    let action_row_box = gtk::Box::new(Orientation::Horizontal, 0);
    action_row_box.set_margin_top(6);
    action_row_box.set_margin_bottom(6);
    action_row_box.set_margin_start(6);
    action_row_box.set_margin_end(6);
    action_row_box.append(&action_box);
    action_row.set_child(Some(&action_row_box));
    flow_group.add(&action_row);

    let capture_group = adw::PreferencesGroup::builder()
        .title("Captura")
        .description("Presets de imagem do notebook. O zoom do preview fica no dock principal.")
        .build();
    capture_group.add(&controls.preset_row);

    let image_group = adw::PreferencesGroup::builder()
        .title("Imagem")
        .description("Ajustes diretos no frame do preview.")
        .build();
    image_group.add(&slider_row("Brilho", &controls.brightness_scale, &controls.brightness_value));
    image_group.add(&slider_row(
        "Exposição (EV)",
        &controls.exposure_scale,
        &controls.exposure_value,
    ));
    image_group.add(&slider_row(
        "Contraste",
        &controls.contrast_scale,
        &controls.contrast_value,
    ));
    image_group.add(&slider_row(
        "Saturação",
        &controls.saturation_scale,
        &controls.saturation_value,
    ));
    image_group.add(&slider_row("Matiz", &controls.hue_scale, &controls.hue_value));
    image_group.add(&slider_row(
        "Nitidez",
        &controls.sharpness_scale,
        &controls.sharpness_value,
    ));

    let color_group = adw::PreferencesGroup::builder()
        .title("Cor")
        .description("Temperatura, tinta e ganho por canal.")
        .build();
    color_group.add(&slider_row(
        "Temperatura",
        &controls.temperature_scale,
        &controls.temperature_value,
    ));
    color_group.add(&slider_row("Tinta", &controls.tint_scale, &controls.tint_value));
    color_group.add(&slider_row("Vermelho", &controls.red_scale, &controls.red_value));
    color_group.add(&slider_row("Verde", &controls.green_scale, &controls.green_value));
    color_group.add(&slider_row("Azul", &controls.blue_scale, &controls.blue_value));
    color_group.add(&slider_row("Gamma", &controls.gamma_scale, &controls.gamma_value));

    let video_group = adw::PreferencesGroup::builder()
        .title("Vídeo")
        .description("Áudio, saída e backend do encoder.")
        .build();
    video_group.add(&controls.record_audio_row);
    video_group.add(&controls.audio_source_row);
    let folders_row = adw::ActionRow::builder()
        .title("Saídas")
        .subtitle(format!(
            "Fotos em {}\nVídeos em {}",
            photo_library_dir().display(),
            video_library_dir().display()
        ))
        .build();
    video_group.add(&folders_row);
    let encoder_row = adw::ActionRow::builder()
        .title("Encoder")
        .subtitle(encoder_backend.ui_label())
        .build();
    video_group.add(&encoder_row);

    page.add(&flow_group);
    page.add(&capture_group);
    page.add(&image_group);
    page.add(&color_group);
    page.add(&video_group);

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(420)
        .child(&page)
        .build()
}

fn build_combo_row(title: &str, subtitle: Option<&str>, values: &[String]) -> adw::ComboRow {
    let string_list = gtk::StringList::new(&[]);
    for value in values {
        string_list.append(value);
    }

    let row = adw::ComboRow::builder().title(title).build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    row.set_model(Some(&string_list));
    row
}

fn build_scale(min: f64, max: f64, step: f64) -> (gtk::Scale, gtk::Label) {
    let scale = gtk::Scale::with_range(Orientation::Horizontal, min, max, step);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.set_valign(Align::Center);

    let value = gtk::Label::new(Some("0.00"));
    value.set_width_chars(5);
    value.set_xalign(1.0);
    value.add_css_class("numeric");

    (scale, value)
}

fn slider_row(title: &str, scale: &gtk::Scale, value: &gtk::Label) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);

    let content = gtk::Box::new(Orientation::Vertical, 8);
    content.add_css_class("camera-slider-row");

    let header = gtk::Box::new(Orientation::Horizontal, 12);
    header.add_css_class("camera-slider-header");

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);

    value.add_css_class("camera-slider-value");

    header.append(&title_label);
    header.append(value);
    content.append(&header);
    content.append(scale);

    row.set_child(Some(&content));
    row
}

pub fn set_scale_value(scale: &gtk::Scale, label: &gtk::Label, value: f64) {
    scale.set_value(value);
    label.set_label(&format!("{value:.2}"));
}

pub fn selected_audio_index(options: &[AudioSourceOption], selected_id: &str) -> u32 {
    options
        .iter()
        .position(|option| option.id == selected_id)
        .unwrap_or(0) as u32
}
