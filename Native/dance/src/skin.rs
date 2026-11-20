//! YACE's original vector noteskin, adapted to meshes on YARG highways.
//! Source: YACE ea87fa3, src/studio/dance_preview.rs (MIT).
use serde::Serialize;

// Exact outline of YACE's M0-30 ... Z path, in its original SVG coordinates.
const ARROW: [[f32; 2]; 9] = [
    [0., -30.],
    [30., 0.],
    [19., 11.],
    [12., 4.],
    [12., 29.],
    [-12., 29.],
    [-12., 4.],
    [-19., 11.],
    [-30., 0.],
];
const MINE: [[f32; 2]; 16] = [
    [0., -26.],
    [7., -17.],
    [18., -18.],
    [17., -7.],
    [26., 0.],
    [17., 7.],
    [18., 18.],
    [7., 17.],
    [0., 26.],
    [-7., 17.],
    [-18., 18.],
    [-17., 7.],
    [-26., 0.],
    [-17., -7.],
    [-18., -18.],
    [-7., -17.],
];
pub const RECEPTOR_Z: f32 = 1.25;
pub const SCROLL_FACTOR: f32 = 0.035;

#[derive(Default, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Mesh {
    // x, y, z, red, green, blue, alpha, note-tint weight.
    pub vertices: Vec<[f32; 8]>,
    pub triangles: Vec<u32>,
}
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Skin {
    pub arrow: Mesh,
    pub receptor: Mesh,
    pub lift: Mesh,
    pub mine: Mesh,
    pub rotations: [f32; 4],
    pub receptor_z: f32,
    pub scroll_factor: f32,
}

pub fn color(beat: f64) -> u32 {
    let row = (beat * 48.).round() as i64;
    for (step, color) in [
        (48, 0xff627d),
        (24, 0x59bcff),
        (16, 0xbe8aff),
        (12, 0xf6d66a),
        (8, 0xef8fdd),
        (6, 0xffab65),
        (4, 0x69e0dc),
        (3, 0x92df88),
    ] {
        if row % step == 0 {
            return color;
        }
    }
    0xc6d7ea
}
pub fn position(distance: f32, note_speed: f32) -> f32 {
    RECEPTOR_Z - distance * note_speed * SCROLL_FACTOR
}
fn cross(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
// Ear clipping preserves the notches in the concave YACE arrow and mine.
fn triangulate(points: &[[f32; 2]]) -> Vec<u32> {
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    let mut result = Vec::new();
    while remaining.len() > 2 {
        let ear = (0..remaining.len())
            .find(|&i| {
                let a = remaining[(i + remaining.len() - 1) % remaining.len()];
                let b = remaining[i];
                let c = remaining[(i + 1) % remaining.len()];
                cross(points[a], points[b], points[c]) > 0.001
                    && !remaining.iter().any(|&p| {
                        p != a
                            && p != b
                            && p != c
                            && cross(points[a], points[b], points[p]) >= 0.
                            && cross(points[b], points[c], points[p]) >= 0.
                            && cross(points[c], points[a], points[p]) >= 0.
                    })
            })
            .expect("noteskin polygons must be simple and counterclockwise");
        let a = remaining[(ear + remaining.len() - 1) % remaining.len()];
        let b = remaining[ear];
        let c = remaining[(ear + 1) % remaining.len()];
        result.extend([a as u32, b as u32, c as u32]);
        remaining.remove(ear);
    }
    result
}
impl Mesh {
    fn vertex(&mut self, p: [f32; 2], scale: f32, layer: f32, rgb: u32, alpha: f32, tint: f32) {
        self.vertices.push([
            p[0] / 160. * scale,
            0.015 + layer * 0.001,
            -p[1] / 160. * scale,
            ((rgb >> 16) & 255) as f32 / 255.,
            ((rgb >> 8) & 255) as f32 / 255.,
            (rgb & 255) as f32 / 255.,
            alpha,
            tint,
        ]);
    }
    fn fill(
        &mut self,
        points: &[[f32; 2]],
        scale: f32,
        layer: f32,
        rgb: u32,
        alpha: f32,
        tint: f32,
    ) {
        let base = self.vertices.len() as u32;
        for &p in points {
            self.vertex(p, scale, layer, rgb, alpha, tint);
        }
        self.triangles
            .extend(triangulate(points).iter().map(|i| base + i));
    }
    fn ring(
        &mut self,
        points: &[[f32; 2]],
        scales: (f32, f32),
        layer: f32,
        rgb: u32,
        alpha: f32,
        tint: f32,
    ) {
        let base = self.vertices.len() as u32;
        for &p in points {
            self.vertex(p, scales.0, layer, rgb, alpha, tint);
            self.vertex(p, scales.1, layer, rgb, alpha, tint);
        }
        for i in 0..points.len() as u32 {
            let a = base + i * 2;
            let b = base + (i + 1) % points.len() as u32 * 2;
            self.triangles.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, layer: f32, rgb: u32, alpha: f32) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        let (x, y) = (-dy / len * width / 2., dx / len * width / 2.);
        self.fill(
            &[
                [a[0] - x, a[1] - y],
                [b[0] - x, b[1] - y],
                [b[0] + x, b[1] + y],
                [a[0] + x, a[1] + y],
            ],
            1.,
            layer,
            rgb,
            alpha,
            0.,
        );
    }
}
fn circle(radius: f32) -> Vec<[f32; 2]> {
    (0..32)
        .map(|i| {
            let angle = i as f32 * std::f32::consts::TAU / 32.;
            [radius * angle.cos(), radius * angle.sin()]
        })
        .collect()
}
pub fn create() -> Skin {
    let mut arrow = Mesh::default();
    arrow.fill(&ARROW, 1., 0., 0x03080d, 1., 0.);
    arrow.fill(&ARROW, 0.95, 1., 0x9bb8d0, 1., 0.);
    arrow.fill(&ARROW, 0.85, 2., 0xffffff, 1., 1.);
    // The same light-to-dark sheen as YACE, expressed as vertex colors.
    let start = arrow.vertices.len();
    arrow.fill(&ARROW, 0.72, 3., 0xffffff, 1., 1.);
    for v in &mut arrow.vertices[start..] {
        let light = (0.75 + v[2] * 2.2).clamp(0.4, 1.);
        v[3] = light;
        v[4] = light;
        v[5] = light;
    }
    arrow.line([-21., -1.], [0., -22.], 2., 4., 0xffffff, 0.6);
    arrow.line([0., -22.], [21., -1.], 2., 4., 0xffffff, 0.6);
    arrow.line([-6., 4.], [-6., 21.], 2., 4., 0xffffff, 0.6);
    let mut lift = Mesh::default();
    lift.fill(&ARROW, 1., 0., 0x0b111a, 1., 0.);
    lift.ring(&ARROW, (1., 0.88), 1., 0xffffff, 1., 1.);
    lift.ring(&ARROW, (0.77, 0.74), 2., 0xffffff, 0.5, 1.);
    let mut receptor = Mesh::default();
    receptor.fill(&ARROW, 1.12, 0., 0x0b111a, 1., 0.);
    receptor.ring(&ARROW, (1.12, 1.02), 1., 0x54677b, 1., 0.);
    receptor.fill(&ARROW, 0.79, 2., 0xffffff, 1., 1.);
    receptor.ring(&ARROW, (0.79, 0.72), 3., 0xd6e7f5, 0.6, 0.);
    let mut mine = Mesh::default();
    mine.fill(&MINE, 1., 0., 0xff7d8e, 1., 0.);
    mine.fill(&MINE, 0.9, 1., 0x3a202d, 1., 0.);
    mine.fill(&circle(15.), 1., 2., 0xf3a17c, 1., 0.);
    mine.fill(&circle(13.), 1., 3., 0x111823, 1., 0.);
    mine.line([-5., -5.], [5., 5.], 3., 4., 0xffb8bb, 1.);
    mine.line([-5., 5.], [5., -5.], 3., 4., 0xffb8bb, 1.);
    Skin {
        arrow,
        receptor,
        lift,
        mine,
        rotations: [-90., 180., 0., 90.],
        receptor_z: RECEPTOR_Z,
        scroll_factor: SCROLL_FACTOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_yace_subdivision_palette() {
        for (row, expected) in [
            (0, 0xff627d),
            (24, 0x59bcff),
            (16, 0xbe8aff),
            (12, 0xf6d66a),
            (8, 0xef8fdd),
            (6, 0xffab65),
            (4, 0x69e0dc),
            (3, 0x92df88),
            (1, 0xc6d7ea),
        ] {
            assert_eq!(color(row as f64 / 48.), expected);
        }
    }
    #[test]
    fn arrows_flow_up_to_fixed_receptors_at_every_speed() {
        for speed in [5., 10., 20.] {
            assert!(position(4., speed) < position(2., speed));
            assert!(position(2., speed) < position(0., speed));
            assert_eq!(position(0., speed), RECEPTOR_Z);
        }
    }
    #[test]
    fn concave_outlines_preserve_area_and_all_mesh_indices() {
        for points in [&ARROW[..], &MINE[..]] {
            let area: f32 = (0..points.len())
                .map(|i| {
                    let a = points[i];
                    let b = points[(i + 1) % points.len()];
                    (a[0] * b[1] - b[0] * a[1]) / 2.
                })
                .sum();
            let triangles = triangulate(points);
            let drawn: f32 = triangles
                .chunks(3)
                .map(|t| {
                    cross(
                        points[t[0] as usize],
                        points[t[1] as usize],
                        points[t[2] as usize],
                    ) / 2.
                })
                .sum();
            assert!((drawn - area).abs() < 0.01);
        }
        let skin = create();
        for mesh in [&skin.arrow, &skin.receptor, &skin.lift, &skin.mine] {
            assert!(!mesh.triangles.is_empty());
            assert!(
                mesh.triangles
                    .iter()
                    .all(|i| (*i as usize) < mesh.vertices.len())
            );
            assert!(mesh.vertices.iter().flatten().all(|v| v.is_finite()));
        }
    }
}
