use crate::prelude::*;

// Align textures to a 8x8 grid
const TEXTURE_ALIGNMENT: i32 = 8;

pub struct TextureAtlas {
    free_rects: Vec<Rect>,
    owned_regions: BitSet,
    // Used in packing, stored to reduce allocations
    used_regions: BitSet,
    width: i32,
    height: i32,
}

impl TextureAtlas {
    pub fn new(width: i32, height: i32) -> TextureAtlas {
        TextureAtlas {
            free_rects: vec![Rect {
                x: 0,
                y: 0,
                width,
                height,
            }],
            owned_regions: BitSet::new((width * height) as usize),
            used_regions: BitSet::new((width * height) as usize),
            width,
            height,
        }
    }

    pub fn find(&mut self, width: i32, height: i32) -> Option<Rect> {
        let mut best: Option<(i32, usize)> = None;

        // Align the texture to the grid
        let awidth = ((width + (TEXTURE_ALIGNMENT - 1)) / TEXTURE_ALIGNMENT) * TEXTURE_ALIGNMENT;
        let aheight = ((height + (TEXTURE_ALIGNMENT - 1)) / TEXTURE_ALIGNMENT) * TEXTURE_ALIGNMENT;

        for (idx, free) in self.free_rects.iter().enumerate() {
            let score = (free.width - awidth) * (free.height - aheight);
            // Will it fit the requested size and is it
            // a tighter fit than the previous match we found?
            if score >= 0
                && free.width >= awidth
                && free.height >= aheight
                && best.map_or(true, |v| v.0 > score)
            {
                best = Some((score, idx));
                if score == 0 {
                    // Found a perfect match
                    // no need to continue searching
                    break;
                }
            }
        }

        if let Some(best) = best {
            let mut rect = self.free_rects.remove(best.1);
            // Use the location of the match but our size.
            let ret = Rect {
                x: rect.x,
                y: rect.y,
                width,
                height,
            };

            // Take owner ship of the
            let bx = rect.x / TEXTURE_ALIGNMENT;
            let by = rect.y / TEXTURE_ALIGNMENT;
            let atlas_width = self.width / TEXTURE_ALIGNMENT;
            for yy in 0..(aheight / TEXTURE_ALIGNMENT) {
                for xx in 0..(awidth / TEXTURE_ALIGNMENT) {
                    self.owned_regions
                        .set((bx + xx + (by + yy) * atlas_width) as usize, true);
                }
            }

            // Split up the remaining space to reuse
            if rect.width - awidth > 0 {
                self.free_rects.push(Rect {
                    x: rect.x + awidth,
                    y: rect.y,
                    width: rect.width - awidth,
                    height: rect.height,
                });
                rect.width = awidth;
            }
            if rect.height - aheight > 0 {
                self.free_rects.push(Rect {
                    x: rect.x,
                    y: rect.y + aheight,
                    width: rect.width,
                    height: rect.height - aheight,
                });
            }

            Some(ret)
        } else {
            None
        }
    }

    /// Returns the rectangle to the atlas to be reused
    pub fn free(&mut self, mut r: Rect) {
        // Align the rect to the grid
        r.width = ((r.width + (TEXTURE_ALIGNMENT - 1)) / TEXTURE_ALIGNMENT) * TEXTURE_ALIGNMENT;
        r.height = ((r.height + (TEXTURE_ALIGNMENT - 1)) / TEXTURE_ALIGNMENT) * TEXTURE_ALIGNMENT;
        self.free_rects.push(r);
    }

    /// Attempts to join seperated free regions into larger
    /// ones.
    pub fn pack_empty(&mut self) {
        self.used_regions.clear();
        self.used_regions.or(&self.owned_regions); // Skip owned regions
        let width = self.width / TEXTURE_ALIGNMENT;
        let height = self.height / TEXTURE_ALIGNMENT;

        self.free_rects.clear(); // This will be recreated

        for y in 0..height {
            for x in 0..width {
                // Find a region that hasn't be used by a other rect yet
                let idx = (x + y * width) as usize;
                if self.used_regions.get(idx) {
                    continue;
                }
                self.used_regions.set(idx, true);
                // Extend horizontally until we hit a wall
                let mut rect_width = 1;
                for xx in x + 1..width {
                    let idx = (xx + y * width) as usize;
                    if self.used_regions.get(idx) {
                        break;
                    }
                    self.used_regions.set(idx, true);
                    rect_width += 1;
                }
                // Extend down as far as possible
                let mut rect_height = 1;
                'height_check: for yy in y + 1..height {
                    for xx in x..x + rect_width {
                        let idx = (xx + yy * width) as usize;
                        if self.used_regions.get(idx) {
                            break 'height_check;
                        }
                    }
                    rect_height += 1;
                    for xx in x..x + rect_width {
                        let idx = (xx + yy * width) as usize;
                        self.used_regions.set(idx, true);
                    }
                }
                // Push the new rect
                self.free_rects.push(Rect {
                    x: x * TEXTURE_ALIGNMENT,
                    y: y * TEXTURE_ALIGNMENT,
                    width: rect_width * TEXTURE_ALIGNMENT,
                    height: rect_height * TEXTURE_ALIGNMENT,
                });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
