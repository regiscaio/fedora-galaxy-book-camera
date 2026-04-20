use std::rc::Rc;

use adw::prelude::*;
use galaxybook_camera::{
    default_config_path,
    localized_app_name,
    photo_library_dir,
    tr,
    trf,
    video_library_dir,
    APP_ID,
};
use gtk::glib;
use gtk::prelude::*;
use gtk::{Align, Orientation};
use libadwaita as adw;

pub fn present_about_dialog(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
) {
    let app_name = localized_app_name();
    let dialog = adw::Dialog::builder()
        .title(tr("Sobre"))
        .content_width(520)
        .content_height(620)
        .build();
    let navigation_view = adw::NavigationView::new();
    navigation_view.set_animate_transitions(true);
    navigation_view.set_pop_on_escape(true);

    let header_title = adw::WindowTitle::new(&tr("Sobre"), "");

    let back_button = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text(tr("Voltar"))
        .visible(false)
        .build();
    back_button.add_css_class("flat");

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&header_title));
    header_bar.pack_start(&back_button);

    let details_subpage = build_about_details_subpage();
    let page = adw::PreferencesPage::builder()
        .name("about")
        .title(tr("Sobre"))
        .build();

    let summary_group = adw::PreferencesGroup::new();
    let summary_row = build_about_summary_row(app_name.as_str());
    summary_group.add(&summary_row);

    let author_row = adw::ActionRow::builder()
        .title("Caio Régis")
        .subtitle("@regiscaio")
        .build();
    author_row.set_activatable(false);
    summary_group.add(&author_row);

    let links_group = adw::PreferencesGroup::builder().title(tr("Projeto")).build();
    let website_row = build_uri_row(
        window,
        toast_overlay,
        &tr("Página da web"),
        "https://caioregis.com",
    );
    let repository_row = build_uri_row(
        window,
        toast_overlay,
        &tr("Repositório do projeto"),
        "https://github.com/regiscaio/fedora-galaxy-book-camera",
    );
    let issues_row = build_uri_row(
        window,
        toast_overlay,
        &tr("Relatar problema"),
        "https://github.com/regiscaio/fedora-galaxy-book-camera/issues",
    );
    let details_row = build_suffix_action_row(
        &tr("Detalhes"),
        &tr("Versão, app ID e caminhos usados pelo app."),
        "go-next-symbolic",
        &tr("Abrir detalhes"),
        {
            let navigation_view = navigation_view.clone();
            move || {
                navigation_view.push_by_tag("details");
            }
        },
    );

    links_group.add(&website_row);
    links_group.add(&repository_row);
    links_group.add(&issues_row);
    links_group.add(&details_row);

    page.add(&summary_group);
    page.add(&links_group);

    let about_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(0)
        .child(&page)
        .build();
    let about_page = adw::NavigationPage::with_tag(&about_scroller, &tr("Sobre"), "about");

    navigation_view.add(&about_page);
    navigation_view.add(&details_subpage);
    navigation_view.replace_with_tags(&["about"]);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&navigation_view));

    dialog.set_child(Some(&toolbar_view));

    back_button.connect_clicked({
        let navigation_view = navigation_view.clone();
        move |_| {
            navigation_view.pop();
        }
    });

    navigation_view.connect_visible_page_notify({
        let header_title = header_title.clone();
        let back_button = back_button.clone();
        move |navigation_view| {
            let Some(page) = navigation_view.visible_page() else {
                header_title.set_title(&tr("Sobre"));
                back_button.set_visible(false);
                return;
            };

            header_title.set_title(page.title().as_str());
            back_button.set_visible(navigation_view.previous_page(&page).is_some());
        }
    });

    dialog.present(Some(window));
}

fn build_suffix_action_row<F>(
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

fn build_uri_row(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
    title: &str,
    uri: &'static str,
) -> adw::ActionRow {
    let window = window.clone();
    let toast_overlay = toast_overlay.clone();
    build_suffix_action_row(
        title,
        uri,
        "send-to-symbolic",
        &tr("Abrir link"),
        move || {
            let launcher = gtk::UriLauncher::new(uri);
            let toast_overlay = toast_overlay.clone();
            launcher.launch(
                Some(&window),
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    if let Err(error) = result {
                        toast_overlay.add_toast(adw::Toast::new(&trf(
                            "Falha ao abrir o link: {error}",
                            &[("error", error.to_string())],
                        )));
                    }
                },
            );
        },
    )
}

fn build_about_summary_row(app_name: &str) -> gtk::ListBoxRow {
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
        "<span alpha='55%' size='small'>{}</span>",
        glib::markup_escape_text(&trf(
            "Versão {version}",
            &[("version", env!("CARGO_PKG_VERSION").to_string())],
        ))
    ));
    version_label.set_xalign(0.0);

    title_row.append(&title_label);
    title_row.append(&version_label);

    let description_label = gtk::Label::new(None);
    description_label.set_markup(&format!(
        "<span alpha='55%' size='small'>{}</span>",
        glib::markup_escape_text(&tr(
            "Aplicativo de câmera nativo para Fedora no Galaxy Book.",
        ))
    ));
    description_label.set_xalign(0.0);
    description_label.set_wrap(true);

    text_column.append(&title_row);
    text_column.append(&description_label);

    content.append(&app_icon);
    content.append(&text_column);
    row.set_child(Some(&content));

    row
}

fn build_about_details_subpage() -> adw::NavigationPage {
    let page = adw::PreferencesPage::builder()
        .name("details")
        .title(tr("Detalhes"))
        .build();

    let app_group = adw::PreferencesGroup::builder()
        .title(tr("Aplicativo"))
        .description(tr("Identificação pública e técnica do Galaxy Book Câmera."))
        .build();

    for (title, subtitle) in [
        (tr("Nome"), localized_app_name()),
        (tr("Versão"), env!("CARGO_PKG_VERSION").to_string()),
        (tr("App ID"), APP_ID.to_string()),
        (tr("Desktop ID"), format!("{APP_ID}.desktop")),
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
        .title(tr("Armazenamento"))
        .description(tr("Arquivos locais e diretórios usados pelo aplicativo."))
        .build();

    for (title, subtitle) in [
        (tr("Configuração"), default_config_path().display().to_string()),
        (tr("Fotos"), photo_library_dir().display().to_string()),
        (tr("Vídeos"), video_library_dir().display().to_string()),
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
        .title(tr("Detalhes"))
        .tag("details")
        .child(&scroller)
        .can_pop(true)
        .build()
}
