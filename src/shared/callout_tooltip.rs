use makepad_widgets::*;
live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;
    use crate::shared::styles::*;
    // A tooltip that appears when hovering over certain elements in the RoomScreen,
    // such as reactions or read receipts.
    pub CalloutTooltipInner = <Tooltip> {
        content: <View> {
            flow: Overlay
            width: Fit
            height: Fit

            rounded_view = <RoundedView> {
                width: Fill,
                height: Fit,

                padding: 10,

                draw_bg: {
                    color: #fff,
                    border_width: 1.0,
                    border_color: #D0D5DD,
                    radius: 2.,
                    instance background_color: (#3b444b),
                    // Height of isoceles triangle
                    instance callout_triangle_height: 7.5,
                    instance callout_offset: 15.0,
                    // callout angle in clockwise direction
                    // 0.0 is pointing up,
                    // 90.0 is pointing left, pointing right is not supported
                    // 180.0 is pointing down,
                    // 270.0 is pointing left
                    instance callout_angle: 0.0,
                    fn pixel(self) -> vec4 {
                        let sdf = Sdf2d::viewport(self.pos * self.rect_size);
                        let rect_size = self.rect_size;
                        if self.callout_angle < 0.5 {
                            sdf.box(
                                // Minus 2.0 to overlap the triangle and rectangle
                                self.border_width,
                                (self.callout_triangle_height - 2.0) + self.border_width,
                                rect_size.x - (self.border_width * 2.0) ,
                                rect_size.y - (self.border_width * 2.0) - (self.callout_triangle_height - 2.0),
                                max(1.0, self.radius)
                            )
                            sdf.fill(self.background_color);
                            sdf.translate(self.callout_offset - 2.0 * self.callout_triangle_height, 1.0);
                            // Draw up-pointed arrow triangle
                            sdf.move_to(self.callout_triangle_height * 2.0, self.callout_triangle_height * 1.0);
                            sdf.line_to(0.0, self.callout_triangle_height * 1.0);
                            sdf.line_to(self.callout_triangle_height, 0.0);
                        } else if self.callout_angle < 90.5 || self.callout_angle > 180.5{ // By right, it should 
                            sdf.box(
                                // Minus 2.0 to overlap the triangle and rectangle
                                (self.callout_triangle_height - 2.0) + self.border_width,
                                0.0 + self.border_width,
                                rect_size.x - (self.border_width * 2.0) - (self.callout_triangle_height - 2.0),
                                rect_size.y - (self.border_width * 2.0),
                                max(1.0, self.radius)
                            )
                            sdf.fill(self.background_color);
                            sdf.translate(0.5, self.callout_offset);
                            // Draw left-pointed arrow triangle
                            sdf.move_to(self.callout_triangle_height, 0.0);
                            sdf.line_to(self.callout_triangle_height, self.callout_triangle_height * 2.0);
                            sdf.line_to(0.5, self.callout_triangle_height);
                        } else if self.callout_angle < 180.5 {
                            sdf.box(
                                // Minus 2.0 to overlap the triangle and rectangle
                                self.border_width,
                                self.border_width,
                                rect_size.x - (self.border_width * 2.0) ,
                                rect_size.y - (self.border_width * 2.0) - (self.callout_triangle_height - 2.0),
                                max(1.0, self.radius)
                            )
                            sdf.fill(self.background_color);
                            sdf.translate(self.callout_offset - self.callout_triangle_height, rect_size.y - 2.0);
                            // Draw down-pointed arrow triangle
                            sdf.move_to(self.callout_triangle_height * 2.0, - self.callout_triangle_height * 1.0);
                            sdf.line_to(self.callout_triangle_height, -0.5);
                            sdf.line_to(0.0, 0.0 - self.callout_triangle_height * 1.0);
                        }

                        sdf.close_path();

                        sdf.fill((self.background_color));
                        return sdf.result;
                    }

                }

                tooltip_label = <Label> {
                    width: Fill,
                    height: Fit,
                    draw_text: {
                        text_style: <THEME_FONT_REGULAR>{font_size: 9},
                        //text_wrap: Word,
                        text_wrap: Line,
                        color: (COLOR_PRIMARY)
                    }
                }
            }
        }
    }
    pub CalloutTooltip = {{CalloutTooltip}} {
        tooltip = <CalloutTooltipInner> {

        }
    }
}
pub const TOOLTIP_HEIGHT_FOR_TOO_CLOSE_BOTTOM: f64 = 80.0;

pub struct CalloutTooltipOptions{
    pub widget_rect: Rect, 
    pub tooltip_width: f64,
    pub color: Option<Vec4>
}

#[derive(Live, LiveHook, Widget)]
pub struct CalloutTooltip {
    #[deref] view: View,
    #[rust] window_size: Option<DVec2>
}

impl Widget for CalloutTooltip {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.widget_match_event(cx, event, scope);
        self.view.handle_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl WidgetMatchEvent for CalloutTooltip {
    fn handle_actions(&mut self, _cx: &mut Cx, actions: &Actions, scope: &mut Scope) {
        if self.window_size.is_none() {
            let app_state = scope.data.get::<crate::app::AppState>().unwrap();
            if let Some(ref window_geom) = app_state.window_geom {
                self.window_size = Some(window_geom.inner_size);
            }
        }
        for action in actions {
            if let WindowAction::WindowGeomChange(window_geom) = action.as_widget_action().cast() {
                self.window_size = Some(window_geom.new_geom.inner_size);
            }
        }
    }
}
impl CalloutTooltip {
    /// Shows a tooltip with the given text and options. The tooltip is positioned
    /// above the widget that the `widget_rect` parameter refers to, unless the
    /// tooltip would be too close to the right or bottom of the window, in which
    /// case it is positioned differently. The `color` parameter sets the color of
    /// the background of the tooltip, and defaults to black if `None`.
    ///
    /// The `too_close_to_right` and `too_close_to_bottom` flags are used to determine
    /// how the tooltip should be positioned. If `too_close_to_right` is true, the tooltip
    /// is positioned to the left of the widget. If `too_close_to_bottom` is true, the
    /// tooltip is positioned above the widget. If both flags are true, the tooltip is
    /// positioned above and to the left of the widget.
    ///
    /// The `callout_offset` parameter is used to calculate the position of the callout
    /// triangle. If `too_close_to_right` is true, the callout offset is calculated as
    /// the width of the tooltip minus the width of the widget divided by 2. If
    /// `too_close_to_right` is false, the callout offset is simply 10.0.
    ///
    /// The `callout_angle` parameter is used to calculate the angle of the callout
    /// triangle. The angle is in the clockwise direction, and is calculated as
    /// follows: if `too_close_to_right` is true, the angle is 0.0. If `too_close_to_bottom`
    /// is true, the angle is 180.0. Otherwise, the angle is 270.0.
    ///
    /// The `tooltip_width` parameter is used to set the width of the tooltip.
    ///
    /// The `window_size` parameter is used to get the size of the window.
    pub fn show_with_options(&mut self, cx: &mut Cx, text: &str, options: CalloutTooltipOptions) {
        let mut too_close_to_right = false;
        let mut too_close_to_bottom = false;
        let Some(window_size) = self.window_size else { println!("no window size"); return };
        let CalloutTooltipOptions{
            widget_rect,
            tooltip_width,
            color
        } = options;
        if (widget_rect.pos.x + widget_rect.size.x) + tooltip_width > window_size.x {
            too_close_to_right = true;
        }
        if (widget_rect.pos.y + widget_rect.size.y) + TOOLTIP_HEIGHT_FOR_TOO_CLOSE_BOTTOM > window_size.y {
            too_close_to_bottom = true;
        }
        let mut pos =  if too_close_to_right {
            DVec2 {
                x: widget_rect.pos.x + (widget_rect.size.x - tooltip_width),
                y: widget_rect.pos.y + widget_rect.size.y
            }
        } else {
            DVec2 {
                x: widget_rect.pos.x + widget_rect.size.x,
                y: widget_rect.pos.y - 5.0
            }
        };
        if too_close_to_bottom && !too_close_to_right {
            pos.x = widget_rect.pos.x + (widget_rect.size.x - 10.0) / 2.0;
            pos.y = widget_rect.pos.y - TOOLTIP_HEIGHT_FOR_TOO_CLOSE_BOTTOM + 10.0;
        }
        let callout_offset = if too_close_to_right {
            tooltip_width - (widget_rect.size.x - 10.0) / 2.0
        } else {
            10.0
        };
        let callout_angle = match (too_close_to_right, too_close_to_bottom) {
            (true, true) => 0.0, //point up
            (true, false) => 0.0, // point up
            (false, true) => 180.0, //point down
            (false, false) => 270.0 //point left
        };
        let tooltip = self.view.tooltip(id!(tooltip));
        tooltip.apply_over(cx, live!(
            content: {
                margin: { left: (pos.x), top: (pos.y)},
                width: (tooltip_width),
                height: Fit,
                rounded_view = {
                    height: Fit,
                    draw_bg: {
                        callout_offset: (callout_offset)
                        // callout angle in clockwise direction
                        callout_angle: (callout_angle)
                    }
                    padding: { left: ( 
                        if callout_angle == 270.0 {
                            10.0 + 7.5 // 7.5 is the height of the isoceles triangle
                        } else {
                            10.0
                        }
                    ), top: ( 
                        if callout_angle == 0.0 {
                            10.0 + 7.5 // 7.5 is the height of the isoceles triangle
                        } else {
                            10.0
                        }
                    )}
                }
            }
        ));
        if too_close_to_bottom {
            tooltip.apply_over(cx, live!(
                content: {
                    height: (TOOLTIP_HEIGHT_FOR_TOO_CLOSE_BOTTOM),
                    width: Fill
                    rounded_view = {
                        height: (TOOLTIP_HEIGHT_FOR_TOO_CLOSE_BOTTOM - 10.0),
                    }
                }
            ));
        }
        if let Some(mut tooltip) = tooltip.borrow_mut() {
            tooltip.set_text(cx, text);
        };
        if let Some(color) = color {
            tooltip.apply_over(cx, live!(
                content: {
                    rounded_view = {
                        draw_bg: {
                            background_color: (color)
                        }
                    }
                }
            ));
        }
        tooltip.show(cx);
    }

    /// Shows the tooltip.
    fn show(&self, cx: &mut Cx) {
        self.view.tooltip(id!(tooltip)).show(cx);
    }
    /// Hide the tooltip.
    fn hide(&self, cx: &mut Cx) {
        self.view.tooltip(id!(tooltip)).hide(cx);
    }
}

impl CalloutTooltipRef {
    /// See [`CalloutTooltip::show_with_options()`].
    pub fn show_with_options(&mut self, cx: &mut Cx, text: &str, options: CalloutTooltipOptions) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.show_with_options(cx, text, options);
        }
    }
    /// See [`CalloutTooltip::show()`].
    pub fn show(&self, cx: &mut Cx) {
        if let Some(inner) = self.borrow_mut() {
            inner.show(cx);
        }
    }
    /// See [`CalloutTooltip::hide()`].
    pub fn hide(&self, cx: &mut Cx) {
        if let Some(inner) = self.borrow_mut() {
            inner.hide(cx);
        }
    }
}
