use crate::presentation::{BackgroundScale, Gravity, Slide};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn background_rect(
    slide: &Slide,
    stage_width: f32,
    stage_height: f32,
    background_width: f32,
    background_height: f32,
) -> Rect {
    if background_width <= 0.0 || background_height <= 0.0 {
        return Rect::default();
    }

    let width_scale = stage_width / background_width;
    let height_scale = stage_height / background_height;
    let (scale_x, scale_y) = match slide.background_scale {
        BackgroundScale::Fill => {
            let scale = width_scale.max(height_scale);
            (scale, scale)
        }
        BackgroundScale::Fit => {
            let scale = width_scale.min(height_scale);
            (scale, scale)
        }
        BackgroundScale::Unscaled => {
            let scale = width_scale.min(height_scale).min(1.0);
            (scale, scale)
        }
        BackgroundScale::Stretch => (width_scale, height_scale),
    };
    let width = background_width * scale_x;
    let height = background_height * scale_y;
    let x = match slide.background_position {
        Gravity::Right | Gravity::TopRight | Gravity::BottomRight => stage_width * 0.95 - width,
        Gravity::Left | Gravity::TopLeft | Gravity::BottomLeft => stage_width * 0.05,
        _ => (stage_width - width) / 2.0,
    };
    let y = match slide.background_position {
        Gravity::Bottom | Gravity::BottomLeft | Gravity::BottomRight => {
            stage_height * 0.95 - height
        }
        Gravity::Top | Gravity::TopLeft | Gravity::TopRight => stage_height * 0.05,
        _ => (stage_height - height) / 2.0,
    };
    Rect {
        x,
        y,
        width,
        height,
    }
}

pub fn text_rect(
    slide: &Slide,
    stage_width: f32,
    stage_height: f32,
    text_width: f32,
    text_height: f32,
) -> (Rect, f32) {
    if text_width <= 0.0 || text_height <= 0.0 {
        return (Rect::default(), 1.0);
    }

    let scale = (stage_width / text_width * 0.8)
        .min(stage_height / text_height * 0.8)
        .min(1.0);
    let width = text_width * scale;
    let height = text_height * scale;
    let x = match slide.text_position {
        Gravity::Right | Gravity::TopRight | Gravity::BottomRight => stage_width * 0.95 - width,
        Gravity::Left | Gravity::TopLeft | Gravity::BottomLeft => stage_width * 0.05,
        _ => (stage_width - width) / 2.0,
    };
    let y = match slide.text_position {
        Gravity::Bottom | Gravity::BottomLeft | Gravity::BottomRight => {
            stage_height * 0.95 - height
        }
        Gravity::Top | Gravity::TopLeft | Gravity::TopRight => stage_height * 0.05,
        _ => (stage_height - height) / 2.0,
    };
    (
        Rect {
            x,
            y,
            width,
            height,
        },
        scale,
    )
}

pub fn shading_rect(stage_width: f32, text: Rect) -> Rect {
    let padding = stage_width * 0.01;
    Rect {
        x: text.x - padding,
        y: text.y - padding,
        width: text.width + padding * 2.0,
        height: text.height + padding * 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_fit_fill_and_stretch_match_current_contract() {
        let mut slide = Slide::default();
        assert_eq!(
            background_rect(&slide, 800.0, 600.0, 1600.0, 900.0),
            Rect {
                x: 0.0,
                y: 75.0,
                width: 800.0,
                height: 450.0,
            }
        );
        slide.background_scale = BackgroundScale::Fill;
        let fill = background_rect(&slide, 800.0, 600.0, 1600.0, 900.0);
        assert!((fill.x + 133.333_34).abs() < 0.001);
        assert_eq!(fill.y, 0.0);
        assert!((fill.width - 1_066.666_7).abs() < 0.001);
        assert_eq!(fill.height, 600.0);
        slide.background_scale = BackgroundScale::Stretch;
        assert_eq!(
            background_rect(&slide, 800.0, 600.0, 1600.0, 900.0),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            }
        );
    }

    #[test]
    fn gravity_and_shading_match_current_contract() {
        let mut slide = Slide {
            text_position: Gravity::BottomRight,
            background_position: Gravity::TopLeft,
            background_scale: BackgroundScale::Unscaled,
            ..Slide::default()
        };
        assert_eq!(
            background_rect(&slide, 1000.0, 500.0, 200.0, 100.0),
            Rect {
                x: 50.0,
                y: 25.0,
                width: 200.0,
                height: 100.0,
            }
        );
        let (text, scale) = text_rect(&slide, 1000.0, 500.0, 400.0, 100.0);
        assert_eq!(scale, 1.0);
        assert_eq!(text.x, 550.0);
        assert_eq!(text.y, 375.0);
        assert_eq!(
            shading_rect(1000.0, text),
            Rect {
                x: 540.0,
                y: 365.0,
                width: 420.0,
                height: 120.0,
            }
        );
        slide.text_position = Gravity::Center;
        let (_, constrained) = text_rect(&slide, 100.0, 100.0, 400.0, 100.0);
        assert_eq!(constrained, 0.2);
    }
}
