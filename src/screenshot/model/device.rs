use eframe::egui::{Pos2, Rect, Vec2};
use image::RgbaImage;
use std::sync::Arc;
use xcap::Monitor;

/// 显示器信息
/// 记录每个显示器的名称、位置和物理尺寸
#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 捕获的屏幕快照
/// 包含原始图像数据和对应的显示器信息
#[derive(Clone)]
pub struct CapturedScreen {
    pub raw_image: Arc<RgbaImage>,
    pub screen_info: MonitorInfo,
}

/// 设备信息
/// 记录多显示器环境下的物理坐标偏移量，用于坐标转换
#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    /// 所有显示器中最小的物理 X 坐标
    pub phys_min_x: i32,
    /// 所有显示器中最小的物理 Y 坐标
    pub phys_min_y: i32,
}

impl DeviceInfo {
    /// 从系统加载显示器信息
    /// 遍历所有显示器，计算虚拟桌面空间的最小物理坐标起点
    pub fn load() -> Self {
        let xcap_monitors = Monitor::all().unwrap_or_else(|err| {
            tracing::error!("获取显示器信息失败: {}", err);
            vec![]
        });

        let mut phys_min_x = i32::MAX;
        let mut phys_min_y = i32::MAX;

        if xcap_monitors.is_empty() {
            return Self::default();
        }

        // 计算所有显示器的最小物理起点
        for m in xcap_monitors {
            let x = m.x().unwrap_or(0);
            let y = m.y().unwrap_or(0);

            if x < phys_min_x {
                phys_min_x = x;
            }
            if y < phys_min_y {
                phys_min_y = y;
            }
        }

        Self {
            phys_min_x,
            phys_min_y,
        }
    }

    /// 将某个物理屏幕的绝对坐标，转换为大画布内的相对逻辑坐标
    ///
    /// # 参数
    /// - `screen`: 目标显示器的信息
    /// - `scale`: 缩放因子（通常为 pixels_per_point）
    ///
    /// # 返回
    /// 该显示器在逻辑坐标系中的矩形区域
    pub fn screen_logical_rect(&self, screen: &MonitorInfo, scale: f32) -> Rect {
        let phys_rel_x = (screen.x - self.phys_min_x) as f32;
        let phys_rel_y = (screen.y - self.phys_min_y) as f32;

        let logic_x = phys_rel_x / scale;
        let logic_y = phys_rel_y / scale;
        let logic_w = screen.width as f32 / scale;
        let logic_h = screen.height as f32 / scale;

        Rect::from_min_size(Pos2::new(logic_x, logic_y), Vec2::new(logic_w, logic_h))
    }
}

/// 获取屏幕的物理边界矩形
#[inline]
pub fn get_screen_phys_rect(info: &MonitorInfo) -> Rect {
    Rect::from_min_size(
        Pos2::new(info.x as f32, info.y as f32),
        egui::vec2(info.width as f32, info.height as f32),
    )
}

/// 根据物理坐标，查找包含该坐标的屏幕物理矩形
///
/// 遍历所有已捕获的屏幕，找到包含给定物理坐标的屏幕并返回其物理矩形
pub fn find_target_screen_rect(captures: &[CapturedScreen], pos: Pos2) -> Option<Rect> {
    captures.iter().find_map(|cap| {
        let rect = get_screen_phys_rect(&cap.screen_info);
        if rect.contains(pos) { Some(rect) } else { None }
    })
}
