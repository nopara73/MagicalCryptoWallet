//! Bounded first-party luminance finder and sampler for camera frames.
//! No frame, sampled matrix or decoded payment is persisted by this module.
use super::matrix::{self, Decoded};
use super::{Control, Error, Result};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub struct Image<'a> {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub luminance: &'a [u8],
}

#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}
impl Point {
    fn distance(self, other: Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}
#[derive(Clone, Copy)]
struct Finder {
    center: Point,
    module: f64,
    hits: usize,
}
struct Binary {
    width: usize,
    height: usize,
    dark: Vec<bool>,
}
impl Binary {
    fn at(&self, x: usize, y: usize) -> bool {
        self.dark[y * self.width + x]
    }
    fn sample(&self, point: Point) -> Option<bool> {
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.0
            || point.y < 0.0
            || point.x >= self.width as f64
            || point.y >= self.height as f64
        {
            return None;
        }
        Some(self.at(point.x as usize, point.y as usize))
    }
}

fn invalid() -> Error {
    Error::NoSymbol
}

fn threshold(image: &Image<'_>, local: bool, control: Control<'_>) -> Result<Binary> {
    control.check()?;
    let mut histogram = [0u64; 256];
    for row in image.luminance.chunks(image.stride).take(image.height) {
        control.check()?;
        for &value in &row[..image.width] {
            histogram[value as usize] += 1;
        }
    }
    let total = (image.width * image.height) as u64;
    let sum = histogram
        .iter()
        .enumerate()
        .map(|(value, &count)| value as u64 * count)
        .sum::<u64>();
    let (mut count, mut partial, mut best, mut cut) = (0u64, 0u64, 0.0, 128u8);
    for (value, &frequency) in histogram.iter().enumerate() {
        count += frequency;
        partial += value as u64 * frequency;
        if count == 0 || count == total {
            continue;
        }
        let delta = partial as f64 / count as f64 - (sum - partial) as f64 / (total - count) as f64;
        let score = count as f64 * (total - count) as f64 * delta * delta;
        if score > best {
            best = score;
            cut = value as u8;
        }
    }
    let mut dark = Vec::with_capacity(total as usize);
    if local {
        let stride = image.width + 1;
        let mut integral = vec![0u32; stride * (image.height + 1)];
        for y in 0..image.height {
            control.check()?;
            let mut row_sum = 0u32;
            for x in 0..image.width {
                row_sum += u32::from(image.luminance[y * image.stride + x]);
                integral[(y + 1) * stride + x + 1] = integral[y * stride + x + 1] + row_sum;
            }
        }
        let radius = (image.width.min(image.height) / 24).clamp(8, 64);
        for y in 0..image.height {
            control.check()?;
            for x in 0..image.width {
                let x0 = x.saturating_sub(radius);
                let y0 = y.saturating_sub(radius);
                let x1 = (x + radius + 1).min(image.width);
                let y1 = (y + radius + 1).min(image.height);
                // A 4096-square image fits in u32, but the sum of two prefix
                // cells can exceed it. Widen before the rectangle query.
                let sum = u64::from(integral[y1 * stride + x1])
                    + u64::from(integral[y0 * stride + x0])
                    - u64::from(integral[y0 * stride + x1])
                    - u64::from(integral[y1 * stride + x0]);
                let mean = sum / ((x1 - x0) * (y1 - y0)) as u64;
                dark.push(u64::from(image.luminance[y * image.stride + x]) + 7 < mean);
            }
        }
    } else {
        for y in 0..image.height {
            control.check()?;
            for x in 0..image.width {
                dark.push(image.luminance[y * image.stride + x] <= cut);
            }
        }
    }
    Ok(Binary {
        width: image.width,
        height: image.height,
        dark,
    })
}

fn finder_ratio(runs: [usize; 5]) -> Option<f64> {
    let total = runs.iter().sum::<usize>();
    if total < 7 || runs.contains(&0) {
        return None;
    }
    let module = total as f64 / 7.0;
    for (run, ratio) in runs.into_iter().zip([1.0, 1.0, 3.0, 1.0, 1.0]) {
        if (run as f64 - module * ratio).abs() > module * ratio * 0.4 {
            return None;
        }
    }
    Some(module)
}

fn cross(
    center: usize,
    limit: usize,
    expected: f64,
    at: impl Fn(usize) -> bool,
) -> Option<(f64, f64)> {
    if center >= limit || !at(center) {
        return None;
    }
    let bound = (expected * 12.0 + 1.0) as usize;
    let start = center.saturating_sub(bound);
    let end = (center + bound + 1).min(limit);
    let mut left = center;
    let mut right = center + 1;
    while left > start && at(left - 1) {
        left -= 1;
    }
    while right < end && at(right) {
        right += 1;
    }
    let middle = (left + right) as f64 / 2.0;
    let mut runs = [0, 0, right - left, 0, 0];
    for (color, index) in [(false, 1), (true, 0)] {
        while left > start && at(left - 1) == color {
            runs[index] += 1;
            left -= 1;
        }
    }
    for (color, index) in [(false, 3), (true, 4)] {
        while right < end && at(right) == color {
            runs[index] += 1;
            right += 1;
        }
    }
    let module = finder_ratio(runs)?;
    if left == start || right == end || !(0.5..=1.8).contains(&(module / expected)) {
        return None;
    }
    Some((middle, module))
}

fn finders(image: &Binary, control: Control<'_>) -> Result<Vec<Finder>> {
    let mut result: Vec<Finder> = Vec::new();
    let mut runs = Vec::with_capacity(image.width);
    for y in 0..image.height {
        control.check()?;
        runs.clear();
        let mut start = 0;
        let mut color = image.at(0, y);
        for x in 1..=image.width {
            if x == image.width || image.at(x, y) != color {
                runs.push((color, start, x - start));
                start = x;
                color = !color;
            }
        }
        for group in runs.windows(5) {
            if !group[0].0 {
                continue;
            }
            let Some(module) = finder_ratio(std::array::from_fn(|i| group[i].2)) else {
                continue;
            };
            let center_x = group[2].1 as f64 + group[2].2 as f64 / 2.0;
            let x = center_x as usize;
            let Some((center_y, vertical)) = cross(y, image.height, module, |v| image.at(x, v))
            else {
                continue;
            };
            let Some((center_x, horizontal)) =
                cross(x, image.width, vertical, |u| image.at(u, center_y as usize))
            else {
                continue;
            };
            let center = Point {
                x: center_x,
                y: center_y,
            };
            let module = (vertical + horizontal) / 2.0;
            if let Some(existing) = result.iter_mut().find(|finder| {
                finder.center.distance(center) < module * 2.0
                    && (0.5..=1.8).contains(&(finder.module / module))
            }) {
                let weight = existing.hits as f64;
                existing.center.x = (existing.center.x * weight + center.x) / (weight + 1.0);
                existing.center.y = (existing.center.y * weight + center.y) / (weight + 1.0);
                existing.module = (existing.module * weight + module) / (weight + 1.0);
                existing.hits += 1;
            } else {
                let candidate = Finder {
                    center,
                    module,
                    hits: 1,
                };
                if result.len() < 128 {
                    result.push(candidate);
                } else if let Some((index, _)) = result
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, finder)| finder.hits)
                    && result[index].hits == 1
                {
                    result[index] = candidate;
                }
            }
        }
    }
    result.retain(|finder| finder.hits >= (finder.module * 1.5).max(2.0) as usize);
    result.retain_mut(|finder| {
        // Crossed run ratios alone also occur in data. Validate the complete
        // nested 7x7 marker before choosing triples, including rotated markers.
        let mut best = (0, 1.0);
        for (sine, cosine) in [
            (0.0, 1.0),
            (0.25881904510252074, 0.9659258262890683),
            (0.5, 0.8660254037844386),
            (
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            (0.8660254037844386, 0.5),
            (0.9659258262890683, 0.25881904510252074),
        ] {
            for scale in [1.0, std::f64::consts::FRAC_1_SQRT_2, 0.85] {
                let mut matches = 0;
                for y in -3i32..=3 {
                    for x in -3i32..=3 {
                        let expected = x.abs().max(y.abs()) != 2;
                        let point = Point {
                            x: finder.center.x
                                + finder.module
                                    * scale
                                    * (f64::from(x) * cosine - f64::from(y) * sine),
                            y: finder.center.y
                                + finder.module
                                    * scale
                                    * (f64::from(x) * sine + f64::from(y) * cosine),
                        };
                        matches += usize::from(image.sample(point) == Some(expected));
                    }
                }
                if matches > best.0 {
                    best = (matches, scale);
                }
            }
        }
        if best.0 >= 43 {
            finder.module *= best.1;
            true
        } else {
            false
        }
    });
    result.sort_by_key(|a| std::cmp::Reverse(a.hits));
    result.truncate(16);
    Ok(result)
}

#[derive(Clone, Copy)]
struct Transform([f64; 8]);
impl Transform {
    fn affine(a: Point, b: Point, c: Point, size: usize) -> Self {
        let span = size as f64 - 7.0;
        let ux = (b.x - a.x) / span;
        let uy = (b.y - a.y) / span;
        let vx = (c.x - a.x) / span;
        let vy = (c.y - a.y) / span;
        Self([
            ux,
            vx,
            a.x - 3.5 * (ux + vx),
            uy,
            vy,
            a.y - 3.5 * (uy + vy),
            0.0,
            0.0,
        ])
    }
    fn perspective(size: usize, points: [Point; 4]) -> Option<Self> {
        let far = size as f64 - 3.5;
        let alignment = size as f64 - 6.5;
        let positions = [(3.5, 3.5), (far, 3.5), (3.5, far), (alignment, alignment)];
        let mut equations = [[0.0; 9]; 8];
        for (index, ((x, y), point)) in positions.into_iter().zip(points).enumerate() {
            equations[2 * index] = [
                x,
                y,
                1.0,
                0.0,
                0.0,
                0.0,
                -point.x * x,
                -point.x * y,
                point.x,
            ];
            equations[2 * index + 1] = [
                0.0,
                0.0,
                0.0,
                x,
                y,
                1.0,
                -point.y * x,
                -point.y * y,
                point.y,
            ];
        }
        for column in 0..8 {
            let pivot = (column..8).max_by(|&a, &b| {
                equations[a][column]
                    .abs()
                    .total_cmp(&equations[b][column].abs())
            })?;
            equations.swap(column, pivot);
            let divisor = equations[column][column];
            if !divisor.is_finite() || divisor.abs() < 1e-8 {
                return None;
            }
            for value in &mut equations[column][column..] {
                *value /= divisor;
            }
            for row in 0..8 {
                if row == column {
                    continue;
                }
                let factor = equations[row][column];
                let pivot_row = equations[column];
                for (cell, pivot) in equations[row][column..]
                    .iter_mut()
                    .zip(&pivot_row[column..])
                {
                    *cell -= factor * pivot;
                }
            }
        }
        Some(Self(std::array::from_fn(|i| equations[i][8])))
    }
    fn map(self, x: f64, y: f64) -> Point {
        let h = self.0;
        let denominator = h[6] * x + h[7] * y + 1.0;
        Point {
            x: (h[0] * x + h[1] * y + h[2]) / denominator,
            y: (h[3] * x + h[4] * y + h[5]) / denominator,
        }
    }
}

fn alignment(
    image: &Binary,
    transform: Transform,
    size: usize,
    control: Control<'_>,
) -> Option<Point> {
    let coordinate = size as f64 - 6.5;
    let predicted = transform.map(coordinate, coordinate);
    let h = transform.0;
    let module = (h[0] * h[0] + h[3] * h[3])
        .sqrt()
        .max((h[1] * h[1] + h[4] * h[4]).sqrt());
    let radius = (module * 6.0 + (size as f64 * module) * 0.1).min(160.0);
    let x0 = (predicted.x - radius).max(0.0) as usize;
    let y0 = (predicted.y - radius).max(0.0) as usize;
    let x1 = ((predicted.x + radius).max(0.0) as usize).min(image.width);
    let y1 = ((predicted.y + radius).max(0.0) as usize).min(image.height);
    let mut best: Option<(f64, Point)> = None;
    for y in y0..y1 {
        if control.check().is_err() {
            return None;
        }
        for x in x0..x1 {
            if !image.at(x, y) {
                continue;
            }
            for scale in [0.75, 1.0, 1.25] {
                let center = Point {
                    x: x as f64 + 0.5,
                    y: y as f64 + 0.5,
                };
                if [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
                    .into_iter()
                    .any(|(u, v)| {
                        image.sample(Point {
                            x: center.x + scale * (u * h[0] + v * h[1]),
                            y: center.y + scale * (u * h[3] + v * h[4]),
                        }) != Some(false)
                    })
                {
                    continue;
                }
                let mut matches = 0;
                for v in -2i32..=2 {
                    for u in -2i32..=2 {
                        let point = Point {
                            x: center.x + scale * (f64::from(u) * h[0] + f64::from(v) * h[1]),
                            y: center.y + scale * (f64::from(u) * h[3] + f64::from(v) * h[4]),
                        };
                        let expected = u.abs().max(v.abs()) != 1;
                        matches += usize::from(image.sample(point) == Some(expected));
                    }
                }
                if matches >= 24 {
                    let score = center.distance(predicted) / module + (25 - matches) as f64 * 3.0;
                    if best.is_none_or(|(previous, _)| score < previous) {
                        best = Some((score, center));
                    }
                }
            }
        }
    }
    best.map(|(_, center)| center)
}

fn sample(image: &Binary, transform: Transform, size: usize) -> Option<Vec<bool>> {
    let mut modules = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let mut dark = 0;
            for dy in [-0.18, 0.0, 0.18] {
                for dx in [-0.18, 0.0, 0.18] {
                    dark += usize::from(
                        image.sample(transform.map(x as f64 + 0.5 + dx, y as f64 + 0.5 + dy))?,
                    );
                }
            }
            modules.push(dark >= 5);
        }
    }
    Some(modules)
}

fn decode_binary(image: &Binary, control: Control<'_>) -> Result<Option<Decoded>> {
    let found = finders(image, control)?;
    let mut triples = Vec::new();
    for i in 0..found.len() {
        for j in i + 1..found.len() {
            for k in j + 1..found.len() {
                let mut triple = [found[i], found[j], found[k]];
                let sides = [
                    triple[0].center.distance(triple[1].center),
                    triple[0].center.distance(triple[2].center),
                    triple[1].center.distance(triple[2].center),
                ];
                let longest = (0..3)
                    .max_by(|&a, &b| sides[a].total_cmp(&sides[b]))
                    .unwrap();
                if longest == 0 {
                    triple.swap(0, 2);
                } else if longest == 1 {
                    triple.swap(0, 1);
                }
                let [a, b, c] = triple;
                let ux = b.center.x - a.center.x;
                let uy = b.center.y - a.center.y;
                let vx = c.center.x - a.center.x;
                let vy = c.center.y - a.center.y;
                let u = (ux * ux + uy * uy).sqrt();
                let v = (vx * vx + vy * vy).sqrt();
                let skew = (ux * vx + uy * vy).abs() / (u * v);
                if !skew.is_finite() || skew > 0.6 || !(0.4..=2.5).contains(&(u / v)) {
                    continue;
                }
                if ux * vy - uy * vx < 0.0 {
                    triple.swap(1, 2);
                }
                let estimate =
                    (u / ((a.module + b.module) / 2.0) + v / ((a.module + c.module) / 2.0)) / 2.0
                        + 7.0;
                if !(17.0..=185.0).contains(&estimate) {
                    continue;
                }
                triples.push((skew + (u.max(v) / u.min(v) - 1.0) * 0.1, estimate, triple));
            }
        }
    }
    triples.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut result: Option<Decoded> = None;
    let mut used = Vec::<Point>::new();
    for (_, estimate, [a, b, c]) in triples.into_iter().take(24) {
        control.check()?;
        if [a, b, c].iter().any(|finder| {
            used.iter()
                .any(|point| point.distance(finder.center) < finder.module * 2.0)
        }) {
            continue;
        }
        let version = ((estimate - 21.0) / 4.0 + 0.5) as i32;
        for offset in [0, -1, 1, -2, 2] {
            let version = version + offset;
            if !(0..40).contains(&version) {
                continue;
            }
            let size = 21 + 4 * version as usize;
            let affine = Transform::affine(a.center, b.center, c.center, size);
            let mut decoded = sample(image, affine, size)
                .and_then(|modules| matrix::decode_modules_control(size, &modules, control).ok());
            if decoded.is_none()
                && size > 21
                && let Some(alignment) = alignment(image, affine, size, control)
                && let Some(perspective) =
                    Transform::perspective(size, [a.center, b.center, c.center, alignment])
            {
                decoded = sample(image, perspective, size).and_then(|modules| {
                    matrix::decode_modules_control(size, &modules, control).ok()
                });
            }
            if let Some(decoded) = decoded {
                if result
                    .as_ref()
                    .is_some_and(|previous| previous.text != decoded.text)
                {
                    return Err(Error::Ambiguous);
                }
                result = Some(decoded);
                used.extend([a.center, b.center, c.center]);
                break;
            }
        }
    }
    Ok(result)
}

pub fn decode(image: Image<'_>) -> Result<Decoded> {
    let cancelled = AtomicBool::new(false);
    decode_control(
        image,
        Control::new(&cancelled, Instant::now() + Duration::from_secs(1)),
    )
}
pub fn decode_control(image: Image<'_>, control: Control<'_>) -> Result<Decoded> {
    control.check()?;
    if image.width < 21
        || image.height < 21
        || image.width > 4096
        || image.height > 4096
        || image.width.saturating_mul(image.height) > 16_777_216
        || image.stride < image.width
        || image.stride > 16384
        || image.luminance.len() != image.stride.saturating_mul(image.height)
    {
        return Err(Error::Invalid("Camera luminance frame exceeds its bounds"));
    }
    // Bounded work on untrusted frames. A camera retries with a new frame;
    // timeout is a failure, never a partial or silently selected payment.
    if let Some(global) = decode_binary(&threshold(&image, false, control)?, control)? {
        return Ok(global);
    }
    control.check()?;
    decode_binary(&threshold(&image, true, control)?, control)?.ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn largest_frame_local_threshold_does_not_overflow() {
        let pixels = vec![255; 4096 * 4096];
        let cancelled = AtomicBool::new(false);
        let image = threshold(
            &Image {
                width: 4096,
                height: 4096,
                stride: 4096,
                luminance: &pixels,
            },
            true,
            Control::new(&cancelled, Instant::now() + Duration::from_secs(30)),
        )
        .unwrap();
        assert!(image.dark.iter().all(|&dark| !dark));
    }

    #[test]
    fn bad_frames_are_bounded_and_blank_frames_fail() {
        for (width, height, stride, len) in [
            (0, 100, 100, 10000),
            (5000, 5000, 5000, 0),
            (30, 30, 29, 870),
            (30, 30, 30, 899),
        ] {
            assert!(
                decode(Image {
                    width,
                    height,
                    stride,
                    luminance: &vec![255; len]
                })
                .is_err()
            );
        }
        assert!(
            decode(Image {
                width: 64,
                height: 64,
                stride: 64,
                luminance: &[255; 4096]
            })
            .is_err()
        );
    }
}
