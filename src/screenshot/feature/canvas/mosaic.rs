use eframe::egui::{Color32, ColorImage, Context, Painter, Pos2, Rect, Vec2};
use image::RgbaImage;
use std::collections::HashSet;

use super::MOSAIC_BLOCK_SIZE;
use crate::screenshot::model::device::get_screen_phys_rect;
use crate::screenshot::feature::screenshot::capture::CapturedScreen;
use crate::screenshot::feature::screenshot::state::MosaicCache;

/// 计算马赛克画笔的物理半径
///
/// 马赛克宽度以逻辑坐标为单位，乘以缩放因子得到物理像素半径。
fn mosaic_radius_phys(mosaic_width: f32, ppp: f32) -> f32 {
    (mosaic_width * ppp) / 2.0
}

/// 收集马赛克轨迹覆盖的网格单元
///
/// 将画笔轨迹上的每个点扩展为半径范围内的网格单元集合。
/// 使用圆形距离判定（含 0.707 对角线补偿）确保覆盖完整。
fn collect_mosaic_grid_cells(
    points: &[Pos2],
    radius_phys: f32,
    block_size_phys: f32,
) -> HashSet<(i32, i32)> {
    let mut grid_cells = HashSet::new();

    for &p_phys in points {
        // 计算该点影响的网格范围
        let min_x = ((p_phys.x - radius_phys) / block_size_phys).floor() as i32;
        let max_x = ((p_phys.x + radius_phys) / block_size_phys).ceil() as i32;
        let min_y = ((p_phys.y - radius_phys) / block_size_phys).floor() as i32;
        let max_y = ((p_phys.y + radius_phys) / block_size_phys).ceil() as i32;

        for cy in min_y..=max_y {
            for cx in min_x..=max_x {
                let cell_center_x = (cx as f32 + 0.5) * block_size_phys;
                let cell_center_y = (cy as f32 + 0.5) * block_size_phys;

                // 圆形距离判定：0.707 ≈ √2/2，补偿网格角落到中心的距离
                if p_phys.distance(Pos2::new(cell_center_x, cell_center_y))
                    <= radius_phys + (block_size_phys * 0.707)
                {
                    grid_cells.insert((cx, cy));
                }
            }
        }
    }

    grid_cells
}

/// 将网格单元裁剪到选区范围内
///
/// 对每个网格单元与选区做矩形交集运算，只保留有正面积的裁剪结果。
fn clip_mosaic_cells(
    grid_cells: HashSet<(i32, i32)>,
    block_size_phys: f32,
    selection: Option<Rect>,
) -> Vec<(i32, i32, Rect)> {
    grid_cells
        .into_iter()
        .filter_map(|(cx, cy)| {
            let cell_rect_phys = Rect::from_min_size(
                Pos2::new(cx as f32 * block_size_phys, cy as f32 * block_size_phys),
                Vec2::splat(block_size_phys),
            );
            let clipped_rect_phys = if let Some(selection) = selection {
                cell_rect_phys.intersect(selection)
            } else {
                cell_rect_phys
            };

            if clipped_rect_phys.is_positive() {
                Some((cx, cy, clipped_rect_phys))
            } else {
                None
            }
        })
        .collect()
}

/// 收集经过选区裁剪的马赛克网格单元
///
/// 组合网格收集和裁剪两个步骤的便捷函数。
fn collect_clipped_mosaic_cells(
    points: &[Pos2],
    mosaic_width: f32,
    ppp: f32,
    block_size_phys: f32,
    selection: Option<Rect>,
) -> Vec<(i32, i32, Rect)> {
    let radius_phys = mosaic_radius_phys(mosaic_width, ppp);
    let grid_cells = collect_mosaic_grid_cells(points, radius_phys, block_size_phys);
    clip_mosaic_cells(grid_cells, block_size_phys, selection)
}

/// 从捕获的屏幕图像中采样指定物理坐标处的颜色
///
/// 遍历所有屏幕捕获，找到包含目标坐标的屏幕，
/// 然后从该屏幕的原始图像中读取对应像素的 RGB 值。
fn sample_mosaic_color(captures: &[CapturedScreen], cell_center_phys: Pos2) -> Color32 {
    for cap in captures {
        let rect = get_screen_phys_rect(&cap.screen_info);
        if rect.contains(cell_center_phys) {
            let local_x = (cell_center_phys.x - rect.min.x) as u32;
            let local_y = (cell_center_phys.y - rect.min.y) as u32;
            if local_x < cap.raw_image.width() && local_y < cap.raw_image.height() {
                let p = cap.raw_image.get_pixel(local_x, local_y);
                return Color32::from_rgb(p[0], p[1], p[2]);
            }
            break;
        }
    }

    Color32::TRANSPARENT
}

/// 实时马赛克渲染（采样原图）
///
/// 在绘制过程中实时显示马赛克效果：
/// 1. 计算轨迹覆盖的网格单元
/// 2. 对每个单元采样原图中心像素颜色
/// 3. 用该颜色填充整个单元方块
pub fn draw_realtime_mosaic(
    painter: &Painter,
    points: &[Pos2],
    mosaic_width: f32,
    global_offset_phys: Pos2,
    ppp: f32,
    selection: Option<Rect>,
    captures: &[CapturedScreen],
) {
    if points.is_empty() {
        return;
    }

    let block_size_phys = MOSAIC_BLOCK_SIZE;
    let clipped_cells =
        collect_clipped_mosaic_cells(points, mosaic_width, ppp, block_size_phys, selection);

    for (cx, cy, clipped_rect_phys) in clipped_cells {
        let phys_x = cx as f32 * block_size_phys;
        let phys_y = cy as f32 * block_size_phys;

        // 采样网格单元中心点的原图颜色
        let cell_center_phys = Pos2::new(
            phys_x + block_size_phys * 0.5,
            phys_y + block_size_phys * 0.5,
        );
        let color = sample_mosaic_color(captures, cell_center_phys);

        if color != Color32::TRANSPARENT {
            // 将物理坐标转换为本地逻辑坐标后渲染方块
            let local_min = Pos2::ZERO + ((clipped_rect_phys.min - global_offset_phys) / ppp);
            let local_rect = Rect::from_min_size(local_min, clipped_rect_phys.size() / ppp);
            painter.rect_filled(local_rect, 0.0, color);
        }
    }
}

/// 生成马赛克纹理缓存
///
/// 将马赛克效果烘焙为一张纹理图像，供后续帧直接使用 GPU 纹理渲染，
/// 避免每帧重复采样原图。返回的 MosaicCache 包含纹理句柄和物理坐标范围。
pub fn generate_mosaic_texture(
    ctx: &Context,
    points: &[Pos2],
    mosaic_width: f32,
    ppp: f32,
    selection: Option<Rect>,
    captures: &[CapturedScreen],
) -> Option<MosaicCache> {
    if points.is_empty() {
        return None;
    }

    let block_size_phys = MOSAIC_BLOCK_SIZE;
    let clipped_cells =
        collect_clipped_mosaic_cells(points, mosaic_width, ppp, block_size_phys, selection);

    if clipped_cells.is_empty() {
        return None;
    }

    // 计算所有裁剪单元的包围盒（物理坐标）
    let min_x_phys = clipped_cells
        .iter()
        .map(|(_, _, rect)| rect.min.x)
        .fold(f32::INFINITY, f32::min);
    let min_y_phys = clipped_cells
        .iter()
        .map(|(_, _, rect)| rect.min.y)
        .fold(f32::INFINITY, f32::min);
    let max_x_phys = clipped_cells
        .iter()
        .map(|(_, _, rect)| rect.max.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y_phys = clipped_cells
        .iter()
        .map(|(_, _, rect)| rect.max.y)
        .fold(f32::NEG_INFINITY, f32::max);

    let width_phys = max_x_phys - min_x_phys;
    let height_phys = max_y_phys - min_y_phys;

    // 创建像素图像（1:1 映射物理像素）
    let img_width = width_phys.ceil() as usize;
    let img_height = height_phys.ceil() as usize;

    if img_width == 0 || img_height == 0 {
        return None;
    }

    let mut pixels: Vec<u8> = vec![0; img_width * img_height * 4];

    // 填充每个网格单元对应的像素块
    for (cx, cy, clipped_rect_phys) in clipped_cells {
        let phys_x = cx as f32 * block_size_phys;
        let phys_y = cy as f32 * block_size_phys;

        let rel_x = clipped_rect_phys.min.x - min_x_phys;
        let rel_y = clipped_rect_phys.min.y - min_y_phys;

        let cell_center_phys = Pos2::new(
            phys_x + block_size_phys * 0.5,
            phys_y + block_size_phys * 0.5,
        );
        let color = sample_mosaic_color(captures, cell_center_phys);

        if color == Color32::TRANSPARENT {
            continue;
        }

        // 将该网格单元覆盖的像素区域填充为采样颜色
        let start_x = rel_x.floor() as usize;
        let start_y = rel_y.floor() as usize;
        let end_x = (rel_x + clipped_rect_phys.width()).ceil() as usize;
        let end_y = (rel_y + clipped_rect_phys.height()).ceil() as usize;
        let end_x = end_x.min(img_width);
        let end_y = end_y.min(img_height);

        for y in start_y..end_y {
            for x in start_x..end_x {
                let idx = (y * img_width + x) * 4;
                if idx + 3 < pixels.len() {
                    pixels[idx] = color.r();
                    pixels[idx + 1] = color.g();
                    pixels[idx + 2] = color.b();
                    pixels[idx + 3] = 255;
                }
            }
        }
    }

    // 创建 egui 纹理
    let color_image = ColorImage::from_rgba_unmultiplied([img_width, img_height], &pixels);
    let texture = ctx.load_texture(
        format!("mosaic_{}_{}", min_x_phys, min_y_phys),
        color_image,
        Default::default(),
    );

    let phys_rect = Rect::from_min_size(
        Pos2::new(min_x_phys, min_y_phys),
        Vec2::new(width_phys, height_phys),
    );

    Some(MosaicCache { texture, phys_rect })
}

/// 将马赛克效果应用到导出的截图图像上
///
/// 在截图保存时调用，直接在 RgbaImage 上写入马赛克像素，
/// 与实时渲染不同，这里使用 1:1 物理像素精度（ppp=1.0）。
pub fn apply_mosaic_to_cropped_image(
    final_image: &mut RgbaImage,
    points: &[Pos2],
    mosaic_width: f32,
    selection_phys: Rect,
) {
    if points.is_empty() {
        return;
    }

    let block_size_phys = MOSAIC_BLOCK_SIZE;
    let clipped_cells = collect_clipped_mosaic_cells(
        points,
        mosaic_width,
        1.0,
        block_size_phys,
        Some(selection_phys),
    );

    if clipped_cells.is_empty() {
        return;
    }

    for (_, _, clipped_rect_phys) in clipped_cells {
        // 从已裁剪的图像中采样中心像素颜色
        let sample_x = clipped_rect_phys.center().x - selection_phys.min.x;
        let sample_y = clipped_rect_phys.center().y - selection_phys.min.y;
        let sample_x = sample_x
            .floor()
            .clamp(0.0, (final_image.width().saturating_sub(1)) as f32)
            as u32;
        let sample_y = sample_y
            .floor()
            .clamp(0.0, (final_image.height().saturating_sub(1)) as f32)
            as u32;
        let pixel = *final_image.get_pixel(sample_x, sample_y);

        // 计算该单元在导出图像中的像素范围
        let start_x = (clipped_rect_phys.min.x - selection_phys.min.x)
            .floor()
            .max(0.0) as u32;
        let start_y = (clipped_rect_phys.min.y - selection_phys.min.y)
            .floor()
            .max(0.0) as u32;
        let end_x = (clipped_rect_phys.max.x - selection_phys.min.x)
            .ceil()
            .min(final_image.width() as f32) as u32;
        let end_y = (clipped_rect_phys.max.y - selection_phys.min.y)
            .ceil()
            .min(final_image.height() as f32) as u32;

        // 用采样颜色填充整个单元
        for y in start_y..end_y {
            for x in start_x..end_x {
                final_image.put_pixel(x, y, pixel);
            }
        }
    }
}
