//! Static deck quality inspection for SVG-authored PowerPoint slides.
//!
//! `create_pptx` cannot rely on the model noticing tiny text or an off-canvas
//! object from source alone. This pass turns the presentation workflow's hard
//! requirements into structured, per-slide diagnostics that the expert prompt
//! can revise before it reports completion.

use serde::Serialize;

use super::{SlideInput, SvgShape};

const DECK_TITLE_MIN_PX: f64 = 50.0 / 0.75;
const SLIDE_TITLE_MIN_PX: f64 = 35.0 / 0.75;
const BODY_MIN_PX: f64 = 16.0 / 0.75;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QualitySeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualityIssue {
    pub severity: QualitySeverity,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slide: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeckQualityReport {
    pub passed: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub issues: Vec<QualityIssue>,
}

#[derive(Debug, Clone)]
struct TextBox {
    text: String,
    font_size: f64,
    bounds: Bounds,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Bounds {
    fn width(self) -> f64 {
        (self.right - self.left).max(0.0)
    }

    fn height(self) -> f64 {
        (self.bottom - self.top).max(0.0)
    }

    fn area(self) -> f64 {
        self.width() * self.height()
    }

    fn intersection(self, other: Self) -> Option<Self> {
        let bounds = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (bounds.width() > 0.0 && bounds.height() > 0.0).then_some(bounds)
    }
}

pub fn inspect_deck(slides: &[SlideInput], speaker_notes: &[String]) -> DeckQualityReport {
    let mut issues = Vec::new();
    if slides.is_empty() {
        push_issue(
            &mut issues,
            QualitySeverity::Error,
            "empty_deck",
            "演示文稿至少需要一页。",
            None,
        );
        return finish(issues);
    }

    let first_ratio = aspect_ratio(&slides[0]);
    if (first_ratio - 16.0 / 9.0).abs() > 0.03 {
        push_issue(
            &mut issues,
            QualitySeverity::Warning,
            "non_widescreen_canvas",
            "画布不是标准 16:9；除非用户明确指定其他比例，否则请使用 1280×720 或 1920×1080。",
            None,
        );
    }

    let mut signatures = Vec::with_capacity(slides.len());
    for (zero_index, slide) in slides.iter().enumerate() {
        let slide_number = zero_index + 1;
        let ratio = aspect_ratio(slide);
        if (ratio - first_ratio).abs() > 0.01 {
            push_issue(
                &mut issues,
                QualitySeverity::Error,
                "inconsistent_canvas",
                "该页画布比例与第一页不一致；整份 PPT 必须共用同一母版尺寸。",
                Some(slide_number),
            );
        }
        inspect_slide(
            slide,
            slide_number,
            speaker_notes.get(zero_index).map(String::as_str),
            &mut issues,
        );
        signatures.push(layout_signature(slide));
    }

    for (offset, window) in signatures.windows(3).enumerate() {
        if window[0] == window[1] && window[1] == window[2] {
            push_issue(
                &mut issues,
                QualitySeverity::Warning,
                "repeated_layout_silhouette",
                "连续三页使用近似相同的构图轮廓；请在全幅视觉、左右分栏、数据主导、时间线等版式之间有意识地变化。",
                Some(offset + 3),
            );
        }
    }

    finish(issues)
}

fn inspect_slide(
    slide: &SlideInput,
    slide_number: usize,
    speaker_note: Option<&str>,
    issues: &mut Vec<QualityIssue>,
) {
    let canvas = Bounds {
        left: slide.content.vb_x,
        top: slide.content.vb_y,
        right: slide.content.vb_x + slide.content.vb_w,
        bottom: slide.content.vb_y + slide.content.vb_h,
    };
    let text_boxes = slide_text_boxes(slide);
    let title_index = text_boxes
        .iter()
        .enumerate()
        .filter(|(_, text)| text.y <= canvas.top + canvas.height() * 0.30)
        .max_by(|(_, left), (_, right)| left.font_size.total_cmp(&right.font_size))
        .map(|(index, _)| index)
        .or_else(|| {
            text_boxes
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| left.font_size.total_cmp(&right.font_size))
                .map(|(index, _)| index)
        });

    if text_boxes.is_empty() {
        push_issue(
            issues,
            QualitySeverity::Warning,
            "missing_visible_claim",
            "本页没有可编辑文本；确认它不是缺少标题或结论的纯装饰页。",
            Some(slide_number),
        );
    }

    for (index, text) in text_boxes.iter().enumerate() {
        let is_title = title_index == Some(index);
        if is_title {
            let minimum = if slide_number == 1 {
                DECK_TITLE_MIN_PX
            } else {
                SLIDE_TITLE_MIN_PX
            };
            if text.font_size + f64::EPSILON < minimum {
                let required_pt = if slide_number == 1 { 50 } else { 35 };
                push_issue(
                    issues,
                    QualitySeverity::Error,
                    "title_too_small",
                    &format!(
                        "主标题“{}”约 {:.1}pt，低于 {}pt；请缩短标题并增大字号。",
                        preview(&text.text),
                        text.font_size * 0.75,
                        required_pt
                    ),
                    Some(slide_number),
                );
            }
            // Reserve breathing room at both sides of a title. A title that
            // consumes more than 75% of the canvas is already fragile across
            // PowerPoint/WPS font substitution even when the SVG estimate is
            // technically still inside the slide.
            if text.bounds.width() > canvas.width() * 0.75 || text.text.contains('\n') {
                push_issue(
                    issues,
                    QualitySeverity::Error,
                    "title_may_wrap",
                    &format!(
                        "主标题“{}”预计无法保持单行；请缩短文案或更换版式，不能靠缩小字号解决。",
                        preview(&text.text)
                    ),
                    Some(slide_number),
                );
            }
        } else if text.font_size + f64::EPSILON < BODY_MIN_PX {
            let is_footer =
                text.y >= canvas.top + canvas.height() * 0.9 && text.text.chars().count() <= 40;
            push_issue(
                issues,
                if is_footer {
                    QualitySeverity::Warning
                } else {
                    QualitySeverity::Error
                },
                "body_text_too_small",
                &format!(
                    "文本“{}”约 {:.1}pt；正文至少 16pt，页脚也应保证投影可读。",
                    preview(&text.text),
                    text.font_size * 0.75
                ),
                Some(slide_number),
            );
        }

        if contains_placeholder(&text.text) {
            push_issue(
                issues,
                QualitySeverity::Error,
                "unresolved_placeholder",
                &format!("仍含占位内容“{}”；交付前必须替换。", preview(&text.text)),
                Some(slide_number),
            );
        }
        if outside_canvas(text.bounds, canvas) {
            push_issue(
                issues,
                QualitySeverity::Error,
                "text_overflow",
                &format!("文本“{}”超出画布或可能被裁切。", preview(&text.text)),
                Some(slide_number),
            );
        }
    }

    for left in 0..text_boxes.len() {
        for right in (left + 1)..text_boxes.len() {
            let Some(overlap) = text_boxes[left]
                .bounds
                .intersection(text_boxes[right].bounds)
            else {
                continue;
            };
            let smaller_area = text_boxes[left]
                .bounds
                .area()
                .min(text_boxes[right].bounds.area())
                .max(1.0);
            let vertical_ratio = overlap.height()
                / text_boxes[left]
                    .bounds
                    .height()
                    .min(text_boxes[right].bounds.height())
                    .max(1.0);
            if overlap.area() / smaller_area > 0.28 && vertical_ratio > 0.45 {
                push_issue(
                    issues,
                    QualitySeverity::Error,
                    "text_overlap",
                    &format!(
                        "文本“{}”与“{}”可能重叠；请调整位置并重新检查。",
                        preview(&text_boxes[left].text),
                        preview(&text_boxes[right].text)
                    ),
                    Some(slide_number),
                );
            }
        }
    }

    let mut has_media = false;
    for shape in &slide.content.shapes {
        if let Some(bounds) = shape_bounds(shape) {
            if outside_canvas(bounds, canvas) {
                push_issue(
                    issues,
                    QualitySeverity::Error,
                    "shape_overflow",
                    "存在超出画布的形状或媒体；请检查全出血意图和裁切边界。",
                    Some(slide_number),
                );
            }
        }
        if let SvgShape::Image {
            width,
            height,
            data,
            ..
        } = shape
        {
            has_media = true;
            if let Some((source_width, source_height)) = raster_dimensions(data) {
                let source_ratio = source_width as f64 / source_height.max(1) as f64;
                let frame_ratio = *width / (*height).max(1.0);
                let mismatch = (source_ratio / frame_ratio).ln().abs();
                if mismatch > 0.12 {
                    push_issue(
                        issues,
                        QualitySeverity::Error,
                        "media_stretched",
                        "媒体框与原图比例不一致，当前会拉伸图像；请先按目标画幅裁切后再嵌入，并检查主体未被切断。",
                        Some(slide_number),
                    );
                }
            } else {
                push_issue(
                    issues,
                    QualitySeverity::Warning,
                    "media_dimensions_unknown",
                    "无法确认媒体原始尺寸；请在最终画布上检查清晰度、比例和裁切。",
                    Some(slide_number),
                );
            }
        }
    }

    if !slide.content.skipped.is_empty() {
        push_issue(
            issues,
            QualitySeverity::Warning,
            "unsupported_svg_elements",
            &format!(
                "转换时跳过了 SVG 元素：{}；请确认没有丢失关键信息。",
                slide.content.skipped.join(", ")
            ),
            Some(slide_number),
        );
    }

    if has_media
        && !speaker_note
            .map(|note| note.contains("[Sources]"))
            .unwrap_or(false)
    {
        push_issue(
            issues,
            QualitySeverity::Error,
            "missing_media_sources",
            "本页包含外部或生成媒体，但 speaker_notes 中没有 [Sources] 来源块。",
            Some(slide_number),
        );
    }
}

fn slide_text_boxes(slide: &SlideInput) -> Vec<TextBox> {
    slide
        .content
        .shapes
        .iter()
        .filter_map(|shape| {
            let SvgShape::Text {
                x,
                y,
                runs,
                font_size,
                text_anchor,
                ..
            } = shape
            else {
                return None;
            };
            let text = runs.iter().map(|run| run.text.as_str()).collect::<String>();
            let font_size = font_size.unwrap_or(18.0);
            let estimated_width = estimated_text_width(&text, font_size);
            let left = match text_anchor.as_str() {
                "middle" => *x - estimated_width / 2.0,
                "end" => *x - estimated_width,
                _ => *x,
            };
            Some(TextBox {
                text,
                font_size,
                y: *y,
                bounds: Bounds {
                    left,
                    top: *y - font_size,
                    right: left + estimated_width,
                    bottom: *y + font_size * 0.25,
                },
            })
        })
        .collect()
}

fn estimated_text_width(text: &str, font_size: f64) -> f64 {
    text.chars()
        .map(|character| {
            if character.is_whitespace() {
                0.3
            } else if character.is_ascii() {
                0.56
            } else {
                1.0
            }
        })
        .sum::<f64>()
        * font_size
}

fn shape_bounds(shape: &SvgShape) -> Option<Bounds> {
    match shape {
        SvgShape::Rect {
            x,
            y,
            width,
            height,
            ..
        }
        | SvgShape::Image {
            x,
            y,
            width,
            height,
            ..
        } => Some(Bounds {
            left: *x,
            top: *y,
            right: *x + *width,
            bottom: *y + *height,
        }),
        SvgShape::Ellipse { cx, cy, rx, ry, .. } => Some(Bounds {
            left: *cx - *rx,
            top: *cy - *ry,
            right: *cx + *rx,
            bottom: *cy + *ry,
        }),
        SvgShape::Line { x1, y1, x2, y2, .. } => Some(Bounds {
            left: x1.min(*x2),
            top: y1.min(*y2),
            right: x1.max(*x2),
            bottom: y1.max(*y2),
        }),
        SvgShape::Text { .. } | SvgShape::Path { .. } => None,
    }
}

fn outside_canvas(bounds: Bounds, canvas: Bounds) -> bool {
    let tolerance = canvas.width().max(canvas.height()) * 0.002;
    bounds.left < canvas.left - tolerance
        || bounds.top < canvas.top - tolerance
        || bounds.right > canvas.right + tolerance
        || bounds.bottom > canvas.bottom + tolerance
}

fn contains_placeholder(text: &str) -> bool {
    let normalized = text.to_lowercase();
    [
        "lorem ipsum",
        "placeholder",
        "replace me",
        "todo",
        "tbd",
        "{{",
        "}}",
        "待补充",
        "占位",
        "示例文本",
        "xxx",
    ]
    .iter()
    .any(|token| normalized.contains(token))
}

fn raster_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((
            u32::from_be_bytes(data[16..20].try_into().ok()?),
            u32::from_be_bytes(data[20..24].try_into().ok()?),
        ));
    }
    if data.len() < 4 || data[0..2] != [0xff, 0xd8] {
        return None;
    }
    let mut index = 2;
    while index + 9 < data.len() {
        if data[index] != 0xff {
            index += 1;
            continue;
        }
        let marker = data[index + 1];
        index += 2;
        if marker == 0xd8 || marker == 0xd9 {
            continue;
        }
        if index + 2 > data.len() {
            break;
        }
        let segment_length = u16::from_be_bytes([data[index], data[index + 1]]) as usize;
        if segment_length < 2 || index + segment_length > data.len() {
            break;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf)
            && segment_length >= 7
        {
            let height = u16::from_be_bytes([data[index + 3], data[index + 4]]) as u32;
            let width = u16::from_be_bytes([data[index + 5], data[index + 6]]) as u32;
            return Some((width, height));
        }
        index += segment_length;
    }
    None
}

fn layout_signature(slide: &SlideInput) -> String {
    let mut images = 0usize;
    let mut text_left = 0usize;
    let mut text_right = 0usize;
    let centre = slide.content.vb_x + slide.content.vb_w / 2.0;
    for shape in &slide.content.shapes {
        match shape {
            SvgShape::Image { .. } => images += 1,
            SvgShape::Text { x, .. } if *x < centre => text_left += 1,
            SvgShape::Text { .. } => text_right += 1,
            _ => {}
        }
    }
    format!(
        "{}:{}:{}:{}",
        images.min(2),
        text_left.min(3),
        text_right.min(3),
        (slide.content.shapes.len() / 5).min(4)
    )
}

fn aspect_ratio(slide: &SlideInput) -> f64 {
    slide.content.vb_w / slide.content.vb_h.max(1.0)
}

fn preview(text: &str) -> String {
    let mut value: String = text.trim().chars().take(28).collect();
    if text.trim().chars().count() > 28 {
        value.push('…');
    }
    value
}

fn push_issue(
    issues: &mut Vec<QualityIssue>,
    severity: QualitySeverity,
    code: &str,
    message: &str,
    slide: Option<usize>,
) {
    issues.push(QualityIssue {
        severity,
        code: code.to_string(),
        message: message.to_string(),
        slide,
    });
}

fn finish(issues: Vec<QualityIssue>) -> DeckQualityReport {
    let error_count = issues
        .iter()
        .filter(|issue| issue.severity == QualitySeverity::Error)
        .count();
    let warning_count = issues.len() - error_count;
    DeckQualityReport {
        passed: error_count == 0,
        error_count,
        warning_count,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::pptx::parse_svg;

    fn slide(index: usize, svg: &str) -> SlideInput {
        SlideInput {
            source_path: format!("slide-{index}.svg"),
            slide_index: index,
            content: parse_svg(svg).unwrap(),
        }
    }

    #[test]
    fn catches_small_wrapping_title_overflow_and_placeholder() {
        let deck = vec![slide(
            1,
            r#"<svg viewBox="0 0 1280 720"><text x="1200" y="80" font-size="30">TODO placeholder title that is far too long to remain on one line</text><text x="80" y="180" font-size="12">body</text></svg>"#,
        )];
        let report = inspect_deck(&deck, &[]);
        let codes: Vec<_> = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect();
        assert!(codes.contains(&"title_too_small"));
        assert!(codes.contains(&"title_may_wrap"));
        assert!(codes.contains(&"unresolved_placeholder"));
        assert!(codes.contains(&"text_overflow"));
        assert!(codes.contains(&"body_text_too_small"));
    }

    #[test]
    fn detects_text_overlap_and_repeated_layouts() {
        let svg = r#"<svg viewBox="0 0 1280 720"><text x="80" y="90" font-size="72">A clear claim</text><text x="80" y="200" font-size="28">First body line</text><text x="80" y="205" font-size="28">Second body line</text></svg>"#;
        let deck = vec![slide(1, svg), slide(2, svg), slide(3, svg)];
        let report = inspect_deck(&deck, &[]);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "text_overlap"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "repeated_layout_silhouette"));
    }
}
