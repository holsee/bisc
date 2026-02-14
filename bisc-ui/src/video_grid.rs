//! Responsive grid layout for arranging multiple [`VideoSurface`](super::video_surface::VideoSurface) widgets.
//!
//! Given a peer count and available bounds, computes a grid of equally-sized cells
//! that best fills the space.

use iced::{Rectangle, Size};

/// Compute grid dimensions (columns, rows) for a given peer count.
fn grid_dims(count: usize) -> (usize, usize) {
    match count {
        0 => (0, 0),
        1 => (1, 1),
        2 => (2, 1),
        3..=4 => (2, 2),
        5..=6 => (3, 2),
        7..=9 => (3, 3),
        _ => {
            // For larger counts, compute a near-square grid
            let cols = (count as f64).sqrt().ceil() as usize;
            let rows = count.div_ceil(cols);
            (cols, rows)
        }
    }
}

/// Compute the position and size of each cell in a responsive grid layout.
///
/// Returns a `Vec<Rectangle>` with exactly `count` entries, each describing
/// the position and size of a video surface cell within `bounds`.
///
/// Layout rules:
/// - 1 peer: full area
/// - 2 peers: side by side (50/50 split)
/// - 3-4 peers: 2x2 grid
/// - 5-6 peers: 3x2 grid
/// - 7-9 peers: 3x3 grid
pub fn grid_layout(count: usize, bounds: Size) -> Vec<Rectangle> {
    if count == 0 {
        return Vec::new();
    }

    let (cols, rows) = grid_dims(count);

    let cell_width = bounds.width / cols as f32;
    let cell_height = bounds.height / rows as f32;

    let mut cells = Vec::with_capacity(count);

    for i in 0..count {
        let col = i % cols;
        let row = i / cols;

        cells.push(Rectangle {
            x: col as f32 * cell_width,
            y: row as f32 * cell_height,
            width: cell_width,
            height: cell_height,
        });
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_one_peer_fills_bounds() {
        let bounds = Size::new(800.0, 600.0);
        let cells = grid_layout(1, bounds);

        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].x, 0.0);
        assert_eq!(cells[0].y, 0.0);
        assert_eq!(cells[0].width, 800.0);
        assert_eq!(cells[0].height, 600.0);
    }

    #[test]
    fn layout_two_peers_side_by_side() {
        let bounds = Size::new(800.0, 600.0);
        let cells = grid_layout(2, bounds);

        assert_eq!(cells.len(), 2);
        // Each should be half width, full height
        assert_eq!(cells[0].width, 400.0);
        assert_eq!(cells[0].height, 600.0);
        assert_eq!(cells[0].x, 0.0);

        assert_eq!(cells[1].width, 400.0);
        assert_eq!(cells[1].height, 600.0);
        assert_eq!(cells[1].x, 400.0);

        // Same y
        assert_eq!(cells[0].y, 0.0);
        assert_eq!(cells[1].y, 0.0);
    }

    #[test]
    fn layout_four_peers_2x2_grid() {
        let bounds = Size::new(800.0, 600.0);
        let cells = grid_layout(4, bounds);

        assert_eq!(cells.len(), 4);
        // Each should be half width, half height
        for cell in &cells {
            assert_eq!(cell.width, 400.0);
            assert_eq!(cell.height, 300.0);
        }
        // Check positions
        assert_eq!(cells[0].x, 0.0);
        assert_eq!(cells[0].y, 0.0);
        assert_eq!(cells[1].x, 400.0);
        assert_eq!(cells[1].y, 0.0);
        assert_eq!(cells[2].x, 0.0);
        assert_eq!(cells[2].y, 300.0);
        assert_eq!(cells[3].x, 400.0);
        assert_eq!(cells[3].y, 300.0);
    }

    #[test]
    fn layout_five_peers_3x2_grid() {
        let bounds = Size::new(900.0, 600.0);
        let cells = grid_layout(5, bounds);

        assert_eq!(cells.len(), 5);
        // 3x2 grid: each cell 300x300
        let (cols, rows) = grid_dims(5);
        assert_eq!(cols, 3);
        assert_eq!(rows, 2);

        let cell_w = 900.0 / 3.0;
        let cell_h = 600.0 / 2.0;

        for cell in &cells {
            assert_eq!(cell.width, cell_w);
            assert_eq!(cell.height, cell_h);
        }
    }

    #[test]
    fn all_cells_fit_within_bounds() {
        let bounds = Size::new(1024.0, 768.0);

        for count in 1..=9 {
            let cells = grid_layout(count, bounds);
            assert_eq!(cells.len(), count, "wrong cell count for {count}");

            for (i, cell) in cells.iter().enumerate() {
                assert!(cell.x >= 0.0, "cell {i} x < 0 for count {count}");
                assert!(cell.y >= 0.0, "cell {i} y < 0 for count {count}");
                assert!(
                    cell.x + cell.width <= bounds.width + 0.01,
                    "cell {i} overflows width for count {count}: {} > {}",
                    cell.x + cell.width,
                    bounds.width
                );
                assert!(
                    cell.y + cell.height <= bounds.height + 0.01,
                    "cell {i} overflows height for count {count}: {} > {}",
                    cell.y + cell.height,
                    bounds.height
                );
            }
        }
    }

    #[test]
    fn layout_zero_peers_empty() {
        let cells = grid_layout(0, Size::new(800.0, 600.0));
        assert!(cells.is_empty());
    }

    #[test]
    fn layout_nine_peers_3x3() {
        let bounds = Size::new(900.0, 900.0);
        let cells = grid_layout(9, bounds);

        assert_eq!(cells.len(), 9);
        let (cols, rows) = grid_dims(9);
        assert_eq!(cols, 3);
        assert_eq!(rows, 3);

        for cell in &cells {
            assert_eq!(cell.width, 300.0);
            assert_eq!(cell.height, 300.0);
        }
    }
}
