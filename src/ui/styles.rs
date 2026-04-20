use gtk::gdk;

pub fn draw_preview_grid(
    _area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: i32,
    height: i32,
) {
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.18);
    cr.set_line_width(1.0);
    for fraction in [1.0_f64 / 3.0, 2.0_f64 / 3.0] {
        let x = width as f64 * fraction;
        let y = height as f64 * fraction;

        cr.move_to(x, 0.0);
        cr.line_to(x, height as f64);
        let _ = cr.stroke();

        cr.move_to(0.0, y);
        cr.line_to(width as f64, y);
        let _ = cr.stroke();
    }
}

pub fn apply_application_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .camera-stage {
            background: #111318;
        }

        .camera-preview {
            background: #000000;
        }

        .camera-bottom-bar {
            padding: 16px 20px 20px 20px;
            border-top: 1px solid alpha(currentColor, 0.08);
        }

        .camera-controls-row {
            min-height: 56px;
        }

        .camera-placeholder {
            padding: 24px;
            min-width: 420px;
        }

        .camera-slider-row {
            padding: 10px 12px 12px 12px;
        }

        .camera-slider-header {
            min-height: 22px;
        }

        .camera-slider-value {
            min-width: 4.5em;
        }

        .camera-hud {
            margin-bottom: 6px;
        }

        .camera-mode-box {
            padding: 8px 10px;
            border-radius: 999px;
            background: alpha(#111318, 0.48);
        }

        .camera-mode-button {
            min-width: 42px;
            min-height: 42px;
            padding: 0;
            border-radius: 999px;
            background: transparent;
            border: none;
            color: rgba(255, 255, 255, 0.92);
            box-shadow: none;
        }

        .camera-mode-button:hover {
            background: alpha(#ffffff, 0.12);
        }

        .camera-mode-button-active {
            background: alpha(#ffffff, 0.18);
            color: #ffffff;
        }

        .camera-zoom-button {
            min-width: 42px;
            min-height: 42px;
            padding: 0;
        }

        .camera-zoom-button-label {
            font-weight: 700;
            letter-spacing: -0.01em;
        }

        .camera-zoom-strip {
            min-height: 42px;
            padding: 0;
            border-radius: 999px;
            background: transparent;
        }

        .camera-zoom-choice {
            min-width: 42px;
            min-height: 42px;
            padding: 0;
            border-radius: 999px;
            background: transparent;
            border: none;
            color: rgba(255, 255, 255, 0.92);
            box-shadow: none;
        }

        .camera-zoom-choice-label {
            font-weight: 700;
            letter-spacing: -0.01em;
        }

        .camera-zoom-choice:hover {
            background: alpha(#ffffff, 0.12);
        }

        .camera-zoom-choice-active {
            background: alpha(#ffffff, 0.18);
            color: #ffffff;
        }

        .camera-header-toggle-active {
            background: alpha(#ffffff, 0.12);
            border-radius: 999px;
        }

        .capture-button {
            min-width: 72px;
            min-height: 72px;
            padding: 0;
            border-radius: 999px;
            border: none;
            background: #ffffff;
            box-shadow:
                0 10px 28px alpha(#000000, 0.28),
                0 2px 8px alpha(#000000, 0.24);
        }

        .capture-button-photo,
        .capture-button-video {
            background: #ffffff;
        }

        .capture-button:hover {
            background: #fcfcfd;
        }

        .capture-button:active {
            background: #f0f1f3;
        }

        .capture-button-recording {
            background: #e5484d;
        }

        .capture-button-recording:hover {
            background: #ee5a5f;
        }

        .capture-button-recording:active {
            background: #d83d43;
        }

        .capture-button-glyph {
            min-width: 0;
            min-height: 0;
            border-radius: 999px;
            background: transparent;
        }

        .capture-button-glyph-photo {
            min-width: 0;
            min-height: 0;
            background: transparent;
        }

        .capture-button-glyph-video {
            min-width: 24px;
            min-height: 24px;
            background: #e5484d;
        }

        .capture-button-glyph-recording {
            min-width: 0;
            min-height: 0;
            background: transparent;
        }

        .capture-button {
            margin-top: 2px;
        }

        .camera-countdown-overlay {
            padding: 0;
            background: transparent;
            color: #ffffff;
            font-size: 4rem;
            font-weight: 800;
            text-shadow: 0 2px 12px alpha(#000000, 0.45);
        }
        ",
    );

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
