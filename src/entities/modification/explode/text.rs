use super::*;

/// Wraps one paragraph at `width` using an average character width.
pub(super) fn wrap_paragraph(text: &str, width: f64, char_width: f64) -> Vec<String> {
    if width <= 0.0 || char_width <= 0.0 {
        return vec![text.to_string()];
    }
    let max_chars = ((width / char_width).floor() as usize).max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if candidate.chars().count() > max_chars && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    lines.push(line);
    lines
}

pub(super) fn mtext_axes(mtext: &MText) -> (Vector3, Vector3) {
    let normal = if mtext.normal.length_squared() > 1e-20 {
        mtext.normal.normalize()
    } else {
        Vector3::UNIT_Z
    };
    let fallback = Vector3::new(mtext.rotation.cos(), mtext.rotation.sin(), 0.0);
    let raw_x = mtext.dwg_x_direction.unwrap_or(fallback);
    let projected = raw_x - normal * raw_x.dot(&normal);
    let x_axis = if projected.length_squared() > 1e-20 {
        projected.normalize()
    } else {
        (Matrix3::arbitrary_axis(normal) * Vector3::UNIT_X).normalize()
    };
    let y_axis = normal.cross(&x_axis).normalize();
    (x_axis, y_axis)
}

pub(super) fn mtext_column_layout(
    mtext: &MText,
    line_count: usize,
    line_index: usize,
) -> (usize, usize) {
    let columns = if mtext.column_data.column_type == 0 {
        1
    } else {
        (mtext.column_data.column_count.max(1) as usize).max(1)
    };
    if columns == 1 || line_count == 0 {
        return (0, line_index);
    }
    let lines_per_column = line_count.div_ceil(columns).max(1);
    let mut column = (line_index / lines_per_column).min(columns - 1);
    if mtext.column_data.flow_reversed {
        column = columns - 1 - column;
    }
    (column, line_index % lines_per_column)
}

pub(super) fn explode_mtext(mtext: &MText) -> Vec<EntityType> {
    let formatted = crate::entities::mtext_format::parse_mtext(&mtext.value, false);
    if formatted.paragraphs.iter().any(|paragraph| {
        paragraph.spans.len() > 1 || paragraph.spans.iter().any(|span| !span.is_plain())
    }) {
        return explode_formatted_mtext(mtext, &formatted);
    }
    // One TEXT per line: formatting codes removed, paragraphs and (approximate)
    // word wrapping at the reference rectangle width, aligned by the attachment point.
    let plain = crate::entities::mtext_format::parse_mtext(&mtext.value, true).to_plain_text();
    let height = if mtext.height > 0.0 {
        mtext.height
    } else {
        1.0
    };
    // average character width of typical CAD fonts
    let char_width = height * 0.6;
    let lines: Vec<String> = plain
        .split('\n')
        .flat_map(|paragraph| wrap_paragraph(paragraph, mtext.rectangle_width, char_width))
        .collect();
    let factor = if mtext.line_spacing_factor > 0.0 {
        mtext.line_spacing_factor
    } else {
        1.0
    };
    let spacing = height * 5.0 / 3.0 * factor;
    let total = height + spacing * (lines.len().saturating_sub(1)) as f64;
    let attachment = mtext.attachment_point as i32;
    let column = (attachment - 1).rem_euclid(3);
    let row = (attachment - 1).div_euclid(3).clamp(0, 2);
    // baseline of the first line, measured from the insertion point (y up)
    let first = match row {
        0 => -height,
        1 => -height + total / 2.0,
        _ => -height + total,
    };
    let rotation = mtext
        .dwg_x_direction
        .filter(|direction| direction.x.abs() + direction.y.abs() > 1e-12)
        .map(|direction| direction.y.atan2(direction.x))
        .unwrap_or(mtext.rotation);
    let (x_axis, y_axis) = mtext_axes(mtext);
    let column_width = (mtext.rectangle_width + mtext.column_data.gutter).max(0.0);
    let horizontal = match column {
        0 => TextHorizontalAlignment::Left,
        1 => TextHorizontalAlignment::Center,
        _ => TextHorizontalAlignment::Right,
    };
    let line_count = lines.len();
    lines
        .into_iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, line)| {
            let (column_index, row_index) = mtext_column_layout(mtext, line_count, i);
            let y = first - spacing * row_index as f64;
            let point =
                mtext.insertion_point + x_axis * (column_width * column_index as f64) + y_axis * y;
            EntityType::Text(Text {
                common: inherit_common(&mtext.common),
                value: line,
                insertion_point: point,
                alignment_point: (column != 0).then_some(point),
                height,
                rotation,
                width_factor: 1.0,
                oblique_angle: 0.0,
                style: mtext.style.clone(),
                style_handle: mtext.style_handle,
                horizontal_alignment: horizontal,
                vertical_alignment: TextVerticalAlignment::Baseline,
                normal: mtext.normal,
                thickness: 0.0,
                generation_flags: 0,
            })
        })
        .collect()
}

pub(super) fn explode_formatted_mtext(
    mtext: &MText,
    document: &crate::entities::mtext_format::MTextDocument,
) -> Vec<EntityType> {
    use crate::entities::mtext_format::{MTextColor, MTextScalar};
    let base_height = if mtext.height > 0.0 {
        mtext.height
    } else {
        1.0
    };
    let spacing = base_height * 5.0 / 3.0
        * if mtext.line_spacing_factor > 0.0 {
            mtext.line_spacing_factor
        } else {
            1.0
        };
    let (x_axis, y_axis) = mtext_axes(mtext);
    let column_width = (mtext.rectangle_width + mtext.column_data.gutter).max(0.0);
    let mut result = Vec::new();
    for (paragraph_index, paragraph) in document.paragraphs.iter().enumerate() {
        let mut advance = 0.0;
        let (column_index, row_index) =
            mtext_column_layout(mtext, document.paragraphs.len(), paragraph_index);
        let y = -(row_index as f64) * spacing - base_height;
        let column_offset = x_axis * (column_width * column_index as f64);
        for span in &paragraph.spans {
            let value = span
                .stacking
                .as_ref()
                .map(|stack| stack.to_plain_text())
                .unwrap_or_else(|| span.text.clone());
            if value.is_empty() {
                continue;
            }
            let height = match span.properties.height {
                Some(MTextScalar::Factor(factor)) => (base_height * factor).max(1e-9),
                Some(MTextScalar::Absolute(value)) => value.max(1e-9),
                None => base_height,
            };
            let width_factor = span.properties.width_factor.unwrap_or(1.0).max(1e-9);
            let rotation =
                mtext.rotation + span.properties.oblique_angle.unwrap_or(0.0).to_radians();
            let point = mtext.insertion_point + column_offset + y_axis * y + x_axis * advance;
            let mut text = Text {
                common: inherit_common(&mtext.common),
                value,
                insertion_point: point,
                alignment_point: None,
                height,
                rotation,
                width_factor,
                oblique_angle: 0.0,
                style: mtext.style.clone(),
                style_handle: mtext.style_handle,
                horizontal_alignment: TextHorizontalAlignment::Left,
                vertical_alignment: TextVerticalAlignment::Baseline,
                normal: mtext.normal,
                thickness: 0.0,
                generation_flags: 0,
            };
            if let Some(font) = span.properties.font.as_ref() {
                if !font.name.is_empty() {
                    text.style = font.name.clone();
                    text.style_handle = None;
                }
            }
            if let Some(color) = span.properties.color {
                text.common.color = match color {
                    MTextColor::Index(index) => Color::from_index(index as i16),
                    MTextColor::TrueColor(value) => Color::from_true_color_value(value as i32),
                    MTextColor::ByLayer => Color::ByLayer,
                    MTextColor::ByBlock => Color::ByBlock,
                    MTextColor::None => text.common.color,
                };
            }
            result.push(EntityType::Text(text));
            advance += span.text.chars().count() as f64 * height * 0.6 * width_factor;
        }
    }
    result
}
