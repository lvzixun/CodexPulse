use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Anchor {
    pub monitor: Option<String>,
    /// Distance in logical pixels from the nearest horizontal/vertical work-area edge.
    pub x: f64,
    pub y: f64,
    pub right: bool,
    pub bottom: bool,
}
impl Anchor {
    pub fn valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.x >= 0.0
            && self.y >= 0.0
            && self.monitor.as_ref().is_none_or(|s| s.len() <= 512)
    }
}
#[derive(Debug, Clone)]
pub struct Area {
    pub monitor: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Area {
    pub fn place(&self, anchor: Option<&Anchor>, logical_size: (f64, f64)) -> Placement {
        let width = ((logical_size.0 * self.scale).round() as u32)
            .min(self.width)
            .max(1);
        let height = ((logical_size.1 * self.scale).round() as u32)
            .min(self.height)
            .max(1);
        let (x, y) = anchor
            .filter(|a| a.valid())
            .map_or((24.0, 64.0), |a| (a.x, a.y));
        let right = anchor.is_none_or(|a| a.right);
        let bottom = anchor.is_some_and(|a| a.bottom);
        let max_x = self.x as i64 + self.width.saturating_sub(width) as i64;
        let max_y = self.y as i64 + self.height.saturating_sub(height) as i64;
        let dx = (x * self.scale).round() as i64;
        let dy = (y * self.scale).round() as i64;
        Placement {
            x: (if right {
                max_x.saturating_sub(dx)
            } else {
                (self.x as i64).saturating_add(dx)
            })
            .clamp(self.x as i64, max_x)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            y: (if bottom {
                max_y.saturating_sub(dy)
            } else {
                (self.y as i64).saturating_add(dy)
            })
            .clamp(self.y as i64, max_y)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            width,
            height,
        }
    }
    pub fn capture(&self, p: Placement) -> Anchor {
        let left = (p.x as i64 - self.x as i64).max(0);
        let top = (p.y as i64 - self.y as i64).max(0);
        let right = (self.x as i64 + self.width as i64 - p.x as i64 - p.width as i64).max(0);
        let bottom = (self.y as i64 + self.height as i64 - p.y as i64 - p.height as i64).max(0);
        Anchor {
            monitor: self.monitor.clone(),
            x: left.min(right) as f64 / self.scale,
            y: top.min(bottom) as f64 / self.scale,
            right: right <= left,
            bottom: bottom <= top,
        }
    }
}
pub fn choose<'a>(areas: &'a [Area], anchor: Option<&Anchor>) -> Option<&'a Area> {
    anchor
        .and_then(|a| a.monitor.as_ref())
        .and_then(|name| areas.iter().find(|a| a.monitor.as_ref() == Some(name)))
        .or_else(|| areas.first())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen(name: &str, scale: f64) -> Area {
        Area {
            monitor: Some(name.into()),
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
            scale,
        }
    }
    #[test]
    fn dpi_change_preserves_logical_edge_distance() {
        let a = screen("left", 1.0);
        let anchor = a.capture(Placement {
            x: -360,
            y: 64,
            width: 336,
            height: 268,
        });
        let b = screen("left", 1.5);
        let restored = b.place(Some(&anchor), (336.0, 268.0));
        assert_eq!(restored.x + restored.width as i32, -36);
        assert_eq!(restored.y, 96);
    }
    #[test]
    fn expanding_keeps_edge_anchor_and_collapsing_returns() {
        let a = screen("left", 1.0);
        let compact = a.place(None, (336.0, 268.0));
        let anchor = a.capture(compact);
        let details = a.place(Some(&anchor), (496.0, 700.0));
        assert_eq!(
            details.x + details.width as i32,
            compact.x + compact.width as i32
        );
        assert_eq!(details.y, compact.y);
        assert_eq!(a.place(Some(&anchor), (336.0, 268.0)), compact);
    }
    #[test]
    fn disconnected_monitor_falls_back_and_clamps_to_work_area() {
        let anchor = Anchor {
            monitor: Some("removed".into()),
            x: 10000.0,
            y: 10000.0,
            right: true,
            bottom: true,
        };
        let areas = [screen("primary", 1.0)];
        let a = choose(&areas, Some(&anchor)).unwrap();
        let p = a.place(Some(&anchor), (496.0, 700.0));
        assert_eq!((p.x, p.y), (a.x, a.y));
        assert!(p.x as i64 + p.width as i64 <= a.x as i64 + a.width as i64);
    }
    #[test]
    fn small_remote_desktop_area_restricts_window_size() {
        let a = Area {
            width: 400,
            height: 300,
            ..screen("remote", 2.0)
        };
        let p = a.place(None, (496.0, 700.0));
        assert_eq!((p.width, p.height, p.x, p.y), (400, 300, a.x, a.y));
    }
}
