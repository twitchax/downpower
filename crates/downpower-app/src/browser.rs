//! The browser half, built only with `hydrate`: fetching from `/api`, reading colors off the
//! stylesheet, and putting the barcode's pixels on its canvas.

use chrono::NaiveDate;
use wasm_bindgen::{Clamped, JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{CanvasRenderingContext2d, ContextAttributes2d, Element, HtmlCanvasElement, ImageData, Response};

use downpower_core::Families;
use downpower_core::barcode::Barcode;
use downpower_core::markers::Markers;

use crate::paint::{self, Grid, Palette, Window};

fn js(error: &JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

/// The body at `url`, `None` on a 404.
///
/// # Errors
///
/// When the request fails or answers anything but 200 or 404.
pub async fn fetch(url: &str) -> Result<Option<Vec<u8>>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: Response = JsFuture::from(window.fetch_with_str(url)).await.map_err(|error| js(&error))?.dyn_into().map_err(|error| js(&error))?;
    if response.status() == 404 {
        return Ok(None);
    }
    if !response.ok() {
        return Err(format!("{url} answered {}", response.status()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(|error| js(&error))?).await.map_err(|error| js(&error))?;
    Ok(Some(js_sys::Uint8Array::new(&buffer).to_vec()))
}

/// Today in the viewer's time zone.
#[must_use]
pub fn today() -> NaiveDate {
    let now = js_sys::Date::new_0();
    NaiveDate::from_ymd_opt(i32::try_from(now.get_full_year()).unwrap_or(2026), now.get_month() + 1, now.get_date()).unwrap_or_default()
}

/// The computed background color of the first element under `root` matching `selector`, as sRGB.
/// A computed color keeps the space it was written in (`oklch(…)` for most tokens), so a 1×1
/// canvas paints it and reads the pixel back.
fn color(probe: &CanvasRenderingContext2d, root: &Element, selector: &str) -> Option<[u8; 3]> {
    let element = root.query_selector(selector).ok()??;
    let style = web_sys::window()?.get_computed_style(&element).ok()??;
    probe.clear_rect(0.0, 0.0, 1.0, 1.0);
    probe.set_fill_style_str(&style.get_property_value("background-color").ok()?);
    probe.fill_rect(0.0, 0.0, 1.0, 1.0);
    let pixel = probe.get_image_data(0.0, 0.0, 1.0, 1.0).ok()?.data();
    Some([*pixel.first()?, *pixel.get(1)?, *pixel.get(2)?])
}

/// The palette the swatches under `root` show: one `[data-paint=<family slug>]` per family, plus
/// `track` and `hidden`. Read at every draw, so a theme change takes effect.
#[must_use]
pub fn palette(root: &Element) -> Option<Palette> {
    let canvas: HtmlCanvasElement = web_sys::window()?.document()?.create_element("canvas").ok()?.dyn_into().ok()?;
    canvas.set_width(1);
    canvas.set_height(1);
    let settings = ContextAttributes2d::new();
    settings.set_will_read_frequently(true);
    let probe: CanvasRenderingContext2d = canvas.get_context_with_context_options("2d", &settings).ok()??.dyn_into().ok()?;
    let mut families = [[0; 3]; 8];
    for family in downpower_core::Family::ALL {
        families[usize::from(family.index())] = color(&probe, root, &format!("[data-paint='{}']", family.slug()))?;
    }
    Some(Palette {
        families,
        track: color(&probe, root, "[data-paint='track']")?,
        hidden: color(&probe, root, "[data-paint='hidden']")?,
        marker: color(&probe, root, "[data-paint='marker']")?,
    })
}

/// Draws `window` of `barcode` on `canvas` at the screen's pixel density, `row` CSS pixels per unit.
///
/// # Errors
///
/// When the canvas has no 2D context or refuses the pixels.
pub fn draw(canvas: &HtmlCanvasElement, barcode: &Barcode, window: Window, visible: Families, palette: &Palette, row: f64, markers: Option<&Markers>) -> Result<(), String> {
    let ratio = web_sys::window().map_or(1.0, |window| window.device_pixel_ratio()).max(1.0);
    let device = |css: f64| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a positive pixel count")]
        let pixels = (css * ratio).round().max(1.0) as u32;
        pixels
    };
    let grid = Grid {
        width: device(f64::from(canvas.client_width())),
        pitch: device(row),
        gap: device(1.0),
    };
    let height = grid.pitch * u32::try_from(barcode.units().len()).unwrap_or(0);
    canvas.set_width(grid.width);
    canvas.set_height(height);
    if height == 0 {
        return Ok(());
    }
    let pixels = paint::paint(barcode, window, grid, visible, palette);
    let image = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&pixels), grid.width, height).map_err(|error| js(&error))?;
    let context: CanvasRenderingContext2d = canvas.get_context("2d").map_err(|error| js(&error))?.ok_or("no 2D context")?.dyn_into().map_err(|object| js(&object))?;
    context.put_image_data(&image, 0.0, 0.0).map_err(|error| js(&error))?;
    if let Some(markers) = markers {
        draw_markers(&context, &paint::placed(markers, barcode, window, grid.width), grid, palette.marker);
    }
    Ok(())
}

/// Each placed marker over its cell (Decision 35): an event a one-pixel tick the row's height, a
/// scram a diamond, so the two read apart by shape alone.
fn draw_markers(context: &CanvasRenderingContext2d, placed: &[(u32, usize, bool)], grid: Grid, color: [u8; 3]) {
    let [red, green, blue] = color;
    context.set_fill_style_str(&format!("rgb({red}, {green}, {blue})"));
    let pitch = f64::from(grid.pitch);
    let height = f64::from(grid.pitch.saturating_sub(grid.gap)).max(1.0);
    for (x, row, scram) in placed {
        #[allow(clippy::cast_precision_loss, reason = "a few hundred rows")]
        let top = *row as f64 * pitch;
        let x = f64::from(*x);
        if *scram {
            let (center, middle, half) = (x + 0.5, top + height / 2.0, (height * 0.9).max(2.0));
            context.begin_path();
            context.move_to(center, middle - half);
            context.line_to(center + half, middle);
            context.line_to(center, middle + half);
            context.line_to(center - half, middle);
            context.close_path();
            context.fill();
        } else {
            context.fill_rect(x, top, 1.0, height);
        }
    }
}
