use gpui_kit::base::{h_flex, v_flex, StyledExt, TestSupportExt};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

// --- 1. Project Panel ---

pub struct ProjectPanel {
    focus_handle: FocusHandle,
}

impl ProjectPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

impl EventEmitter<PanelEvent> for ProjectPanel {}

impl Focusable for ProjectPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ProjectPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("project_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Header / search bar & actions
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(
                        h_flex()
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .text_xs()
                            .child("Search Project..."),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_color(cx.theme().foreground)
                                    .text_xs()
                                    .child("+ Bin"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_color(cx.theme().foreground)
                                    .text_xs()
                                    .child("+ Comp"),
                            ),
                    ),
            )
            // Column headers
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(110.)).child("Name"))
                    .child(div().w(px(70.)).child("Type"))
                    .child(div().w(px(70.)).child("Resolution"))
                    .child(div().flex_1().child("Duration")),
            )
            // Asset media list placeholder
            .child(
                v_flex()
                    .id("project_assets")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .px_1()
                    .py_1()
                    // Selected Item: Comp 1
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(cx.theme().accent)
                            .text_color(cx.theme().accent_foreground)
                            .text_xs()
                            .items_center()
                            .child(div().w(px(110.)).font_semibold().child("📁 Comp 1"))
                            .child(div().w(px(70.)).child("Composition"))
                            .child(div().w(px(70.)).child("1920x1080"))
                            .child(div().flex_1().child("00:10:00")),
                    )
                    // Item 2: Video
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .items_center()
                            .child(div().w(px(110.)).child("🎬 Footage_01.mp4"))
                            .child(div().w(px(70.)).child("H.264 Video"))
                            .child(div().w(px(70.)).child("1920x1080"))
                            .child(div().flex_1().child("00:30:00")),
                    )
                    // Item 3: Image
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .items_center()
                            .child(div().w(px(110.)).child("🖼 Background.png"))
                            .child(div().w(px(70.)).child("PNG Image"))
                            .child(div().w(px(70.)).child("3840x2160"))
                            .child(div().flex_1().child("Still")),
                    )
                    // Item 4: Audio
                    .child(
                        h_flex()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .items_center()
                            .child(div().w(px(110.)).child("🔊 Audio_Track.wav"))
                            .child(div().w(px(70.)).child("WAV Audio"))
                            .child(div().w(px(70.)).child("44.1 kHz"))
                            .child(div().flex_1().child("01:15:00")),
                    ),
            )
            // Footer
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("4 items • 1 selected • 30.00 fps"),
            )
    }
}

impl BasePanel for ProjectPanel {
    fn panel_name(&self) -> &'static str {
        "project"
    }
}

impl Panel for ProjectPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Project".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Project"
    }
}

// --- 2. Composition Viewer Panel ---

pub type CompositionPanel = CompositionViewerPanel;

pub struct CompositionViewerPanel {
    focus_handle: FocusHandle,
}

impl CompositionViewerPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

impl EventEmitter<PanelEvent> for CompositionViewerPanel {}

impl Focusable for CompositionViewerPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CompositionViewerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("composition_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Viewport header / controls
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(div().font_bold().child("Comp 1"))
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("1920 x 1080 (1.00)"),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("30.00 fps"),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("100% (Fit)"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("Full Res"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("Active Camera"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("RGB"),
                            ),
                    ),
            )
            // Composition Canvas area
            .child(
                v_flex()
                    .id("composition_viewer")
                    .test_support()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .overflow_hidden()
                    .child(
                        // 16:9 canvas frame placeholder
                        v_flex()
                            .w(px(512.))
                            .h(px(288.))
                            .border_2()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted)
                            .rounded_sm()
                            .items_center()
                            .justify_center()
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(div().font_semibold().text_sm().child("Comp 1"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("1920 x 1080 • 16:9"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("00:00:00:00 / 00:00:10:00"),
                                    ),
                            ),
                    ),
            )
            // Status bar
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .justify_between()
                    .child(div().child("Time: 00:00:00:00 (Frame 0)"))
                    .child(div().child("Scroll to Zoom • Space+Drag to Pan")),
            )
    }
}

impl BasePanel for CompositionViewerPanel {
    fn panel_name(&self) -> &'static str {
        "composition"
    }
}

impl Panel for CompositionViewerPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Composition".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Composition: Comp 1"
    }
}

// --- 3. Properties Panel ---

pub struct PropertiesPanel {
    focus_handle: FocusHandle,
}

impl PropertiesPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

impl EventEmitter<PanelEvent> for PropertiesPanel {}

impl Focusable for PropertiesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PropertiesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("properties_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Header
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .items_center()
                    .justify_between()
                    .child(div().font_semibold().text_xs().child("Selected: Layer 1 (Text)"))
                    .child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_xs()
                            .child("2D Layer"),
                    ),
            )
            // Inspector fields
            .child(
                v_flex()
                    .id("properties_inspector")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .p_3()
                    .gap_3()
                    // Transform Section Header
                    .child(
                        div()
                            .font_semibold()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("▼ Transform"),
                    )
                    // Anchor Point
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                div()
                                    .w(px(70.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Anchor Pt"),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .bg(cx.theme().muted)
                                            .rounded_sm()
                                            .child("X: 960.0"),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .bg(cx.theme().muted)
                                            .rounded_sm()
                                            .child("Y: 540.0"),
                                    ),
                            ),
                    )
                    // Position (X, Y)
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                div()
                                    .w(px(70.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Position"),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .bg(cx.theme().muted)
                                            .rounded_sm()
                                            .child("X: 960.0"),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .bg(cx.theme().muted)
                                            .rounded_sm()
                                            .child("Y: 540.0"),
                                    ),
                            ),
                    )
                    // Scale (X, Y)
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                div()
                                    .w(px(70.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Scale"),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .bg(cx.theme().muted)
                                            .rounded_sm()
                                            .child("100.0 %"),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .bg(cx.theme().muted)
                                            .rounded_sm()
                                            .child("100.0 %"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("🔗"),
                                    ),
                            ),
                    )
                    // Rotation
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                div()
                                    .w(px(70.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Rotation"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().muted)
                                    .rounded_sm()
                                    .child("0x +0.0°"),
                            ),
                    )
                    // Opacity
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                div()
                                    .w(px(70.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Opacity"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().muted)
                                    .rounded_sm()
                                    .child("100.0 %"),
                            ),
                    )
                    // Section 2: Switches & Modes
                    .child(
                        div()
                            .font_semibold()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("▼ Switches & Modes"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().secondary)
                                    .text_color(cx.theme().foreground)
                                    .rounded_sm()
                                    .child("[✓] Visible"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().secondary)
                                    .text_color(cx.theme().foreground)
                                    .rounded_sm()
                                    .child("[✓] Audio"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().muted)
                                    .rounded_sm()
                                    .child("[ ] Solo"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().muted)
                                    .rounded_sm()
                                    .child("[ ] Lock"),
                            ),
                    ),
            )
    }
}

impl BasePanel for PropertiesPanel {
    fn panel_name(&self) -> &'static str {
        "properties"
    }
}

impl Panel for PropertiesPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Properties".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Properties"
    }
}

// --- 4. Effects Panel ---

pub struct EffectsPanel {
    focus_handle: FocusHandle,
}

impl EffectsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

impl EventEmitter<PanelEvent> for EffectsPanel {}

impl Focusable for EffectsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EffectsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("effects_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Search / Filter
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(
                        h_flex()
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .text_xs()
                            .child("Search Effects & Presets..."),
                    ),
            )
            // Effects Category List
            .child(
                v_flex()
                    .id("effects_categories")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .p_2()
                    .gap_1()
                    // Category 1: Blur & Sharpen
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .font_semibold()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("▼ Blur & Sharpen (4)"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Gaussian Blur"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Fast Box Blur"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Directional Blur"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Sharpen"),
                    )
                    // Category 2: Color Correction
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .font_semibold()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("▼ Color Correction (5)"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Curves"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Levels"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Color Balance (HLS)"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Hue / Saturation"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Lumetri Color"),
                    )
                    // Category 3: Distort
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .font_semibold()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("▼ Distort (3)"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Transform"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Ripple"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Displacement Map"),
                    )
                    // Category 4: Generate
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .font_semibold()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("▼ Generate (3)"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Gradient Ramp"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Fill"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Stroke"),
                    )
                    // Category 5: Transition
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .font_semibold()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("▼ Transition (3)"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Linear Wipe"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Radial Wipe"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_0p5()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("• Block Dissolve"),
                    ),
            )
            // Footer
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("18 built-in effects available"),
            )
    }
}

impl BasePanel for EffectsPanel {
    fn panel_name(&self) -> &'static str {
        "effects"
    }
}

impl Panel for EffectsPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Effects".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Effects & Presets"
    }
}

// --- 5. Timeline Panel ---

pub struct TimelinePanel {
    focus_handle: FocusHandle,
}

impl TimelinePanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

impl EventEmitter<PanelEvent> for TimelinePanel {}

impl Focusable for TimelinePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TimelinePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("timeline_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Header / Timecode & Transport
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .items_center()
                    .justify_between()
                    // Timecode display
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().muted)
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .font_bold()
                                    .text_sm()
                                    .text_color(cx.theme().primary)
                                    .child("00:00:00:00"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Frame 0 / 300"),
                            ),
                    )
                    // Transport controls
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_xs()
                                    .child("|<"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_xs()
                                    .child("<"),
                            )
                            .child(
                                div()
                                    .px_3()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().primary)
                                    .text_color(cx.theme().primary_foreground)
                                    .text_xs()
                                    .font_bold()
                                    .child("▶ Play"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_xs()
                                    .child(">"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_xs()
                                    .child(">|"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().secondary)
                                    .text_xs()
                                    .child("Loop: On"),
                            ),
                    )
                    // Duration & In/Out
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(div().child("In: 00:00:00:00"))
                            .child(div().child("Out: 00:00:10:00")),
                    ),
            )
            // Time Ruler
            .child(
                h_flex()
                    .h(px(22.))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .w(px(240.))
                            .px_3()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child("Layer Name / Switches"),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .justify_between()
                            .px_3()
                            .child(div().child("00:00s"))
                            .child(div().child("00:02s"))
                            .child(div().child("00:04s"))
                            .child(div().child("00:06s"))
                            .child(div().child("00:08s"))
                            .child(div().child("00:10s")),
                    ),
            )
            // Tracks area
            .child(
                v_flex()
                    .id("timeline")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    // Track 1
                    .child(
                        h_flex()
                            .h(px(26.))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .child(
                                h_flex()
                                    .w(px(240.))
                                    .px_2()
                                    .border_r_1()
                                    .border_color(cx.theme().border)
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .child(div().child("1  [V] [A] [•]  Layer 1: Text"))
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Normal"),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .px_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(16.))
                                            .rounded_sm()
                                            .bg(cx.theme().primary)
                                            .opacity(0.85)
                                            .px_2()
                                            .text_xs()
                                            .text_color(cx.theme().primary_foreground)
                                            .child("Layer 1: Text [00:00 - 00:10]"),
                                    ),
                            ),
                    )
                    // Track 2
                    .child(
                        h_flex()
                            .h(px(26.))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .child(
                                h_flex()
                                    .w(px(240.))
                                    .px_2()
                                    .border_r_1()
                                    .border_color(cx.theme().border)
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .child(div().child("2  [V] [A] [•]  Footage_01.mp4"))
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Normal"),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .px_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .w(px(380.))
                                            .h(px(16.))
                                            .rounded_sm()
                                            .bg(cx.theme().accent)
                                            .opacity(0.85)
                                            .px_2()
                                            .text_xs()
                                            .text_color(cx.theme().accent_foreground)
                                            .child("Footage_01.mp4 [00:00 - 00:08]"),
                                    ),
                            ),
                    )
                    // Track 3
                    .child(
                        h_flex()
                            .h(px(26.))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .child(
                                h_flex()
                                    .w(px(240.))
                                    .px_2()
                                    .border_r_1()
                                    .border_color(cx.theme().border)
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .child(div().child("3  [V] [ ] [•]  Background.png"))
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Normal"),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .px_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(16.))
                                            .rounded_sm()
                                            .bg(cx.theme().secondary)
                                            .px_2()
                                            .text_xs()
                                            .text_color(cx.theme().foreground)
                                            .child("Background.png [00:00 - 00:10]"),
                                    ),
                            ),
                    )
                    // Track 4
                    .child(
                        h_flex()
                            .h(px(26.))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .child(
                                h_flex()
                                    .w(px(240.))
                                    .px_2()
                                    .border_r_1()
                                    .border_color(cx.theme().border)
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .child(div().child("4  [ ] [A] [•]  Soundtrack.wav"))
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Normal"),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .px_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(16.))
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .px_2()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("♫ Soundtrack.wav [00:00 - 00:10]"),
                                    ),
                            ),
                    ),
            )
    }
}

impl BasePanel for TimelinePanel {
    fn panel_name(&self) -> &'static str {
        "timeline"
    }
}

impl Panel for TimelinePanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Timeline".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Timeline: Comp 1"
    }
}

// --- App Panels Container ---

#[derive(Clone)]
pub struct AppPanels {
    pub project: Entity<ProjectPanel>,
    pub composition: Entity<CompositionViewerPanel>,
    pub viewer: Entity<CompositionViewerPanel>,
    pub properties: Entity<PropertiesPanel>,
    pub effects: Entity<EffectsPanel>,
    pub timeline: Entity<TimelinePanel>,
}

impl AppPanels {
    pub fn new(cx: &mut App) -> Self {
        let composition = cx.new(CompositionViewerPanel::new);
        Self {
            project: cx.new(ProjectPanel::new),
            composition: composition.clone(),
            viewer: composition,
            properties: cx.new(PropertiesPanel::new),
            effects: cx.new(EffectsPanel::new),
            timeline: cx.new(TimelinePanel::new),
        }
    }
}
