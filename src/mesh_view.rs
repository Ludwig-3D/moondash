use crate::{MeshMarker, tools};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};

pub mod gpu;

const WIDTH: u32 = 760;
const HEIGHT: u32 = 412;
const MAX_GRID_SAMPLES: usize = 65;
const SUBDIVISIONS_PER_SAMPLE: usize = 4;

pub struct Surface {
    vertices: Vec<[f32; 3]>,
    faces: Vec<Face>,
    sample_dimensions: [usize; 2],
    extents: [f32; 2],
    mesh_bounds: [Option<(f32, f32)>; 2],
    axis_bounds: [Option<(f32, f32)>; 2],
    machine_axes: [bool; 2],
    low: f64,
    high: f64,
}

struct Face {
    corners: [usize; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GpuVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct MeshGeometry {
    pub triangles: Vec<GpuVertex>,
    pub strokes: Vec<GpuVertex>,
}

#[derive(Default)]
pub struct Scene {
    pub geometry: Option<MeshGeometry>,
    pub revision: u64,
}

impl Scene {
    pub fn set_geometry(&mut self, geometry: Option<MeshGeometry>) {
        self.geometry = geometry;
        self.revision = self.revision.wrapping_add(1);
    }
}

pub type SceneHandle = Rc<RefCell<Scene>>;

#[derive(Clone, Copy)]
struct Point {
    x: f32,
    y: f32,
    depth: f32,
}

impl Surface {
    pub fn from_mesh(mesh: &Value, toolhead: &Value) -> Option<Self> {
        let matrix = tools::bed_mesh_matrix(mesh);
        let (low, high) = tools::mesh_range(&matrix)?;
        let rows = matrix.len();
        let columns = matrix[0].len();
        let mesh_bounds: [Option<(f32, f32)>; 2] = std::array::from_fn(|axis| {
            let min = mesh["mesh_min"][axis].as_f64().map(|n| n as f32);
            let max = mesh["mesh_max"][axis].as_f64().map(|n| n as f32);
            min.zip(max).filter(|(min, max)| valid_bounds(*min, *max))
        });
        let machine_bounds: [Option<(f32, f32)>; 2] = std::array::from_fn(|axis| {
            let min = toolhead["axis_minimum"][axis]
                .as_f64()
                .map(|value| value as f32);
            let max = toolhead["axis_maximum"][axis]
                .as_f64()
                .map(|value| value as f32);
            min.zip(max).filter(|(min, max)| valid_bounds(*min, *max))
        });
        let machine_axes: [bool; 2] = std::array::from_fn(|axis| {
            machine_bounds[axis].is_some_and(|(machine_min, machine_max)| {
                mesh_bounds[axis].is_some_and(|(mesh_min, mesh_max)| {
                    mesh_min >= machine_min - 0.001 && mesh_max <= machine_max + 0.001
                })
            })
        });
        let axis_bounds: [Option<(f32, f32)>; 2] = std::array::from_fn(|axis| {
            if machine_axes[axis] {
                machine_bounds[axis]
            } else {
                mesh_bounds[axis]
            }
        });
        let default_spans = [columns, rows].map(|samples| samples.saturating_sub(1).max(1) as f32);
        let spans = std::array::from_fn(|axis| {
            axis_bounds[axis].map_or(default_spans[axis], |(min, max)| max - min)
        });
        let longest = spans[0].max(spans[1]);
        let extents = spans.map(|span| span / longest);
        let sample_count = |samples: usize| {
            (samples.saturating_sub(1) * SUBDIVISIONS_PER_SAMPLE + 1).clamp(2, MAX_GRID_SAMPLES)
        };
        let width = sample_count(columns);
        let height = sample_count(rows);
        let mut vertices = Vec::with_capacity(width * height);
        let mut heights = Vec::with_capacity(width * height);
        let span = high - low;

        for y in 0..height {
            let t = y as f32 / (height - 1) as f32;
            for x in 0..width {
                let s = x as f32 / (width - 1) as f32;
                let value = interpolate_mesh(&matrix, s, t);
                let z = if span > 0. {
                    ((value - low) / span) as f32 * 0.36
                } else {
                    0.11
                };
                let mapped: [f32; 2] = std::array::from_fn(|axis| {
                    let fraction = [s, t][axis];
                    match (mesh_bounds[axis], axis_bounds[axis]) {
                        (Some((mesh_min, mesh_max)), Some((axis_min, axis_max))) => {
                            let coordinate = mesh_min + (mesh_max - mesh_min) * fraction;
                            ((coordinate - axis_min) / (axis_max - axis_min)).clamp(0., 1.)
                        }
                        _ => fraction,
                    }
                });
                vertices.push([
                    (mapped[0] - 0.5) * extents[0],
                    (mapped[1] - 0.5) * extents[1],
                    z,
                ]);
                heights.push(value);
            }
        }

        let mut faces = Vec::with_capacity((width - 1) * (height - 1) * 2);
        for y in 0..height - 1 {
            for x in 0..width - 1 {
                let first = y * width + x;
                for corners in [
                    [first, first + 1, first + width + 1],
                    [first, first + width + 1, first + width],
                ] {
                    let value = corners.iter().map(|&index| heights[index]).sum::<f64>() / 3.;
                    let (r, g, b) = tools::mesh_color(value, Some((low, high)));
                    faces.push(Face {
                        corners,
                        color: [
                            f32::from(r) / 255.,
                            f32::from(g) / 255.,
                            f32::from(b) / 255.,
                            1.,
                        ],
                    });
                }
            }
        }
        Some(Self {
            vertices,
            faces,
            sample_dimensions: [columns, rows],
            extents,
            mesh_bounds,
            axis_bounds,
            machine_axes,
            low,
            high,
        })
    }

    pub fn render(&self, yaw: f32, pitch: f32) -> (MeshGeometry, Vec<MeshMarker>) {
        self.render_in_view(yaw, pitch, 1., WIDTH as f32, HEIGHT as f32)
    }

    pub fn render_in_view(
        &self,
        yaw: f32,
        pitch: f32,
        zoom: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> (MeshGeometry, Vec<MeshMarker>) {
        let viewport = [viewport_width, viewport_height].map(|size| {
            if size.is_finite() && size > 1. {
                size
            } else {
                1.
            }
        });
        let (sin, cos) = yaw.to_radians().sin_cos();
        let (tilt_sin, tilt_cos) = pitch.to_radians().sin_cos();
        let projected_width = self.extents[0] * cos.abs() + self.extents[1] * sin.abs();
        let projected_height = (self.extents[0] * sin.abs() + self.extents[1] * cos.abs())
            * tilt_sin
            + 0.48 * tilt_cos;
        let scale = ((viewport[0] - 128.).max(1.) / projected_width)
            .min((viewport[1] - 96.).max(1.) / projected_height)
            * zoom.clamp(0.65, 4.);
        let project = |[x, y, z]: [f32; 3]| {
            let forward = x * sin + y * cos;
            Point {
                x: WIDTH as f32 * 0.5 + (x * cos - y * sin) * scale * WIDTH as f32 / viewport[0],
                y: HEIGHT as f32 * 0.5
                    + (forward * tilt_sin - (z - 0.12) * tilt_cos) * scale * HEIGHT as f32
                        / viewport[1],
                depth: forward * tilt_cos + z * tilt_sin,
            }
        };
        let [half_x, half_y] = self.extents.map(|size| size * 0.5);
        let floor = -0.12;
        let mut triangles = Vec::with_capacity(self.faces.len() * 3 + 6);
        let mut strokes = Vec::with_capacity(1024);
        let floor_outline = [
            [-half_x, -half_y, floor],
            [half_x, -half_y, floor],
            [half_x, half_y, floor],
            [-half_x, half_y, floor],
        ]
        .map(project);
        push_polygon(&mut triangles, &floor_outline, [0.39, 0.45, 0.55, 0.11]);
        for index in 0..=10 {
            let t = index as f32 * 0.2 - 1.;
            let major = index % 2 == 0;
            let grid_color = if major {
                [0.76, 0.82, 0.92, 0.78]
            } else {
                [0.58, 0.65, 0.76, 0.52]
            };
            let width = if major { 2.2 } else { 1.6 };
            push_line(
                &mut strokes,
                project([half_x * t, -half_y, floor]),
                project([half_x * t, half_y, floor]),
                grid_color,
                width,
                viewport,
            );
            push_line(
                &mut strokes,
                project([-half_x, half_y * t, floor]),
                project([half_x, half_y * t, floor]),
                grid_color,
                width,
                viewport,
            );
        }

        let points: Vec<_> = self.vertices.iter().copied().map(project).collect();
        for face in &self.faces {
            for corner in face.corners {
                triangles.push(gpu_vertex(points[corner], face.color));
            }
        }

        let mut markers = Vec::with_capacity(10);
        let near_x = if sin >= 0. { half_x } else { -half_x };
        let near_y = if cos >= 0. { half_y } else { -half_y };
        let axis_color = [0.66, 0.73, 0.83, 0.92];
        push_line(
            &mut strokes,
            project([-half_x, near_y, floor]),
            project([half_x, near_y, floor]),
            axis_color,
            2.8,
            viewport,
        );
        push_line(
            &mut strokes,
            project([near_x, -half_y, floor]),
            project([near_x, half_y, floor]),
            axis_color,
            2.8,
            viewport,
        );
        push_line(
            &mut strokes,
            project([near_x, near_y, floor]),
            project([near_x, near_y, 0.36]),
            axis_color,
            2.8,
            viewport,
        );

        let mut mark = |world: [f32; 3], offset: [f32; 2], label: String| {
            let point = project(world);
            let marker_offset = [
                offset[0] * WIDTH as f32 / viewport[0],
                offset[1] * HEIGHT as f32 / viewport[1],
            ];
            push_line(
                &mut strokes,
                point,
                Point {
                    x: point.x + marker_offset[0] * 0.35,
                    y: point.y + marker_offset[1] * 0.35,
                    ..point
                },
                axis_color,
                1.8,
                viewport,
            );
            markers.push(MeshMarker {
                x: ((point.x + marker_offset[0]) / WIDTH as f32).clamp(0., 1.),
                y: ((point.y + marker_offset[1]) / HEIGHT as f32).clamp(0., 1.),
                label: label.into(),
            });
        };
        let x_side = if near_y >= 0. { -sin } else { sin };
        let y_side = if near_x >= 0. { cos } else { -cos };
        let axis_label = |axis: usize, t: f32| {
            let name = if axis == 0 { "X" } else { "Y" };
            if self.machine_axes[axis] {
                let (min, max) = self.axis_bounds[axis].expect("machine range is validated");
                format!("{name} {}", format_coordinate(min + (max - min) * t))
            } else if let Some((min, max)) = self.mesh_bounds[axis] {
                format!("{name} mesh {}", format_coordinate(min + (max - min) * t))
            } else {
                format!(
                    "{name} idx {:.0}",
                    t * self.sample_dimensions[axis].saturating_sub(1) as f32
                )
            }
        };
        for axis in 0..2 {
            for t in [0., 0.5, 1.] {
                let (world, offset) = if axis == 0 {
                    ([(2. * t - 1.) * half_x, near_y, floor], [x_side * 33., 20.])
                } else {
                    ([near_x, (2. * t - 1.) * half_y, floor], [y_side * 33., 20.])
                };
                let at_corner =
                    (axis == 0 && world[0] == near_x) || (axis == 1 && world[1] == near_y);
                if !at_corner {
                    mark(world, offset, axis_label(axis, t));
                }
            }
        }
        let x_t = if near_x >= 0. { 1. } else { 0. };
        let y_t = if near_y >= 0. { 1. } else { 0. };
        mark(
            [near_x, near_y, floor],
            [0., 35.],
            format!("{}  ·  {}", axis_label(0, x_t), axis_label(1, y_t)),
        );
        let side = if project([near_x, near_y, floor]).x >= WIDTH as f32 * 0.5 {
            1.
        } else {
            -1.
        };
        if self.high > self.low {
            mark(
                [near_x, near_y, 0.],
                [side * 49., -2.],
                format!("{:+.3}", self.low),
            );
            mark(
                [near_x, near_y, 0.36],
                [side * 49., -5.],
                format!("Z {:+.3}", self.high),
            );
        } else {
            mark(
                [near_x, near_y, 0.11],
                [side * 49., -5.],
                format!("Z {:+.3}", self.low),
            );
        }

        (MeshGeometry { triangles, strokes }, markers)
    }
}

fn gpu_vertex(point: Point, color: [f32; 4]) -> GpuVertex {
    GpuVertex {
        position: [
            point.x / WIDTH as f32 * 2. - 1.,
            1. - point.y / HEIGHT as f32 * 2.,
            (0.5 - point.depth * 0.2).clamp(0., 1.),
        ],
        color,
    }
}

fn push_line(
    triangles: &mut Vec<GpuVertex>,
    from: Point,
    to: Point,
    color: [f32; 4],
    width: f32,
    viewport: [f32; 2],
) {
    let scale_x = viewport[0] / WIDTH as f32;
    let scale_y = viewport[1] / HEIGHT as f32;
    let dx = (to.x - from.x) * scale_x;
    let dy = (to.y - from.y) * scale_y;
    let length = dx.hypot(dy);
    if length <= f32::EPSILON {
        return;
    }
    let half_width = width * 0.5;
    let ox = -dy / length * half_width / scale_x;
    let oy = dx / length * half_width / scale_y;
    let corners = [
        Point {
            x: from.x + ox,
            y: from.y + oy,
            ..from
        },
        Point {
            x: from.x - ox,
            y: from.y - oy,
            ..from
        },
        Point {
            x: to.x - ox,
            y: to.y - oy,
            ..to
        },
        Point {
            x: to.x + ox,
            y: to.y + oy,
            ..to
        },
    ];
    push_polygon(triangles, &corners, color);
}

fn valid_bounds(min: f32, max: f32) -> bool {
    min.is_finite() && max.is_finite() && min < max && (max - min).is_finite()
}

fn format_coordinate(value: f32) -> String {
    let value = if value.abs() < 0.0005 { 0. } else { value };
    let mut formatted = format!("{value:.3}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

fn push_polygon(triangles: &mut Vec<GpuVertex>, points: &[Point], color: [f32; 4]) {
    for index in 1..points.len() - 1 {
        triangles.extend([
            gpu_vertex(points[0], color),
            gpu_vertex(points[index], color),
            gpu_vertex(points[index + 1], color),
        ]);
    }
}

fn interpolate_mesh(matrix: &[Vec<f64>], s: f32, t: f32) -> f64 {
    let rows = matrix.len();
    let columns = matrix[0].len();
    let x = s.clamp(0., 1.) * (columns - 1) as f32;
    let y = t.clamp(0., 1.) * (rows - 1) as f32;
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(columns - 1);
    let y1 = (y0 + 1).min(rows - 1);
    let fx = f64::from(x - x0 as f32);
    let fy = f64::from(y - y0 as f32);
    let mix = |a: f64, b: f64, fraction: f64| a + (b - a) * fraction;
    mix(
        mix(matrix[y0][x0], matrix[y0][x1], fx),
        mix(matrix[y1][x0], matrix[y1][x1], fx),
        fy,
    )
}

#[cfg(test)]
mod tests {
    use super::{MAX_GRID_SAMPLES, Surface, interpolate_mesh};
    use serde_json::{Value, json};

    fn make_surface(mesh: Value) -> Surface {
        make_surface_with_toolhead(mesh, json!({}))
    }

    fn make_surface_with_toolhead(mesh: Value, toolhead: Value) -> Surface {
        Surface::from_mesh(&mesh, &toolhead).expect("valid bed mesh")
    }

    #[test]
    fn bilinear_interpolation_preserves_samples_and_mixes_between_them() {
        let matrix = vec![vec![0., 2.], vec![4., 6.]];
        assert_eq!(interpolate_mesh(&matrix, 0., 0.), 0.);
        assert_eq!(interpolate_mesh(&matrix, 1., 1.), 6.);
        assert_eq!(interpolate_mesh(&matrix, 0.5, 0.5), 3.);

        let surface = make_surface(json!({"mesh_matrix": matrix}));
        assert_eq!(surface.vertices.len(), 25);
        assert_eq!(surface.faces.len(), 32);
        assert_eq!(surface.vertices[0][2], 0.);
        assert_eq!(surface.vertices[24][2], 0.36);
        assert!((surface.vertices[12][2] - 0.18).abs() < 0.0001);
    }

    #[test]
    fn dense_and_degenerate_grids_produce_bounded_surfaces() {
        let dense = vec![vec![0.25; 80]; 80];
        let surface = make_surface(json!({"mesh_matrix": dense}));
        assert_eq!(surface.vertices.len(), MAX_GRID_SAMPLES * MAX_GRID_SAMPLES);
        assert_eq!(surface.faces.len(), (MAX_GRID_SAMPLES - 1).pow(2) * 2);

        for matrix in [
            vec![vec![0.1]],
            vec![vec![0.1, 0.2, 0.3]],
            vec![vec![0.1], vec![0.2], vec![0.3]],
        ] {
            let surface = make_surface(json!({"mesh_matrix": matrix}));
            assert!(surface.vertices.len() >= 4);
            assert!(!surface.faces.is_empty());
            assert!(surface.vertices.iter().all(|vertex| vertex[2].is_finite()));
        }
        assert!(Surface::from_mesh(&json!({}), &json!({})).is_none());
    }

    #[test]
    fn coordinate_markers_use_printer_bounds_or_grid_indices() {
        let bounded = make_surface(json!({
            "mesh_matrix": [[-0.1, 0.0, 0.1], [0.0, 0.2, 0.3], [0.1, 0.3, 0.5]],
            "mesh_min": [10, 20],
            "mesh_max": [210, 220]
        }));
        let (_, markers) = bounded.render(-34., 37.);
        let labels: Vec<_> = markers.iter().map(|marker| marker.label.as_str()).collect();
        assert!(labels.contains(&"X mesh 110"));
        assert!(labels.contains(&"Y mesh 120"));
        assert!(labels.iter().any(|label| label.starts_with("Z ")));

        let unbounded = make_surface(json!({
            "mesh_matrix": [[0.0, 0.1, 0.2], [0.1, 0.2, 0.3], [0.2, 0.3, 0.4]]
        }));
        let (_, markers) = unbounded.render(-34., 37.);
        let labels: Vec<_> = markers.iter().map(|marker| marker.label.as_str()).collect();
        assert!(labels.contains(&"X idx 1"));
        assert!(labels.contains(&"Y idx 1"));
    }

    #[test]
    fn marker_positions_follow_rotation_and_stay_inside_viewport() {
        let surface = make_surface(json!({
            "mesh_matrix": [[0.0, 0.1, 0.2], [0.1, 0.2, 0.3], [0.2, 0.3, 0.4]],
            "mesh_min": [0, 0],
            "mesh_max": [200, 200]
        }));
        let (_, first) = surface.render(23., 37.);
        let (_, second) = surface.render(103., 52.);
        let first = first
            .iter()
            .find(|marker| marker.label == "X mesh 100")
            .unwrap();
        let second = second
            .iter()
            .find(|marker| marker.label == "X mesh 100")
            .unwrap();
        assert!((first.x - second.x).abs() > 0.01 || (first.y - second.y).abs() > 0.01);

        for marker in [first, second] {
            assert!((0. ..=1.).contains(&marker.x));
            assert!((0. ..=1.).contains(&marker.y));
        }
    }

    #[test]
    fn projected_geometry_has_depth_values_and_rotates() {
        let surface = make_surface(json!({
            "mesh_matrix": [[0.0, 0.1, 0.2], [0.1, 0.2, 0.3], [0.2, 0.3, 0.4]]
        }));
        let (first, _) = surface.render(23., 37.);
        let (second, _) = surface.render(103., 52.);
        assert_eq!(first.triangles.len(), surface.faces.len() * 3 + 6);
        assert!(!first.strokes.is_empty());
        assert!(
            first
                .triangles
                .iter()
                .all(|vertex| (0. ..=1.).contains(&vertex.position[2]))
        );
        assert!(
            first.triangles.iter().any(|vertex| {
                (vertex.position[2] - first.triangles[0].position[2]).abs() > 0.001
            })
        );
        assert_ne!(first.triangles[0].position, second.triangles[0].position);
        assert_eq!(first.strokes.len(), second.strokes.len());
        assert_ne!(first.strokes[0].position, second.strokes[0].position);
    }

    #[test]
    fn projection_fits_wide_and_tall_viewports_and_zoom_is_bounded() {
        let surface = make_surface(json!({
            "mesh_matrix": [[0.0, 0.2], [0.2, 0.4]],
            "mesh_min": [0, 0],
            "mesh_max": [200, 200]
        }));

        for (width, height) in [(1024., 512.), (420., 900.)] {
            let (geometry, markers) = surface.render_in_view(23., 37., 1., width, height);
            assert!(geometry.triangles.iter().all(|vertex| {
                vertex.position[0].abs() <= 1. && vertex.position[1].abs() <= 1.
            }));
            assert!(markers.iter().all(|marker| {
                (0. ..=1.).contains(&marker.x) && (0. ..=1.).contains(&marker.y)
            }));
        }

        let (default, _) = surface.render_in_view(23., 37., 1., 900., 600.);
        let (zoomed, _) = surface.render_in_view(23., 37., 2., 900., 600.);
        let (maximum, _) = surface.render_in_view(23., 37., 4., 900., 600.);
        let (clamped, _) = surface.render_in_view(23., 37., 12., 900., 600.);
        let distance_from_center = |geometry: &super::MeshGeometry| {
            let position = geometry.triangles[0].position;
            (position[0] * position[0] + position[1] * position[1]).sqrt()
        };
        assert!(distance_from_center(&zoomed) > distance_from_center(&default));
        assert!(distance_from_center(&maximum) > distance_from_center(&zoomed));
        assert_eq!(maximum.triangles[0].position, clamped.triangles[0].position);
    }

    #[test]
    fn machine_axis_limits_position_mesh_and_label_fractional_endpoints() {
        let surface = make_surface_with_toolhead(
            json!({
                "mesh_matrix": [[0.0, 0.1, 0.2], [0.1, 0.2, 0.3], [0.2, 0.3, 0.4]],
                "mesh_min": [12.5, 23.25],
                "mesh_max": [212.5, 223.25]
            }),
            json!({
                "axis_minimum": [-10.25, -20.5, 0.0],
                "axis_maximum": [250.75, 240.25, 300.0]
            }),
        );

        let expected_x = (22.75 / 261.) - 0.5;
        let expected_y = ((43.75 / 260.75) - 0.5) * (260.75 / 261.);
        assert!((surface.vertices[0][0] - expected_x).abs() < 0.0001);
        assert!((surface.vertices[0][1] - expected_y).abs() < 0.0001);

        let (_, markers) = surface.render(-34., 37.);
        let labels = markers
            .iter()
            .map(|marker| marker.label.as_str())
            .collect::<Vec<_>>();
        assert!(labels.contains(&"X 250.75"));
        assert!(labels.contains(&"Y -20.5"));
        assert!(labels.contains(&"X -10.25  ·  Y 240.25"));
    }

    #[test]
    fn inconsistent_machine_limits_fall_back_per_axis_and_grid_strokes_are_tessellated() {
        let surface = make_surface_with_toolhead(
            json!({
                "mesh_matrix": [[0.0, 0.1, 0.2], [0.1, 0.2, 0.3], [0.2, 0.3, 0.4]],
                "mesh_min": [10, 20],
                "mesh_max": [210, 220]
            }),
            json!({
                "axis_minimum": [-100, -50, 0],
                "axis_maximum": [100, 250, 300]
            }),
        );
        let (geometry, markers) = surface.render(23., 37.);
        let labels = markers
            .iter()
            .map(|marker| marker.label.as_str())
            .collect::<Vec<_>>();

        assert!(labels.iter().any(|label| label.contains("X mesh 210")));
        assert!(labels.contains(&"Y -50"));
        assert!(geometry.strokes.len() >= 22 * 6);
        assert_eq!(geometry.strokes.len() % 3, 0);
        assert!(geometry.strokes.iter().any(|vertex| vertex.color[3] > 0.7));
    }

    #[test]
    fn machine_limits_without_mesh_coordinates_do_not_claim_machine_positions() {
        let surface = make_surface_with_toolhead(
            json!({"mesh_matrix": [[0.0, 0.1], [0.2, 0.3]]}),
            json!({
                "axis_minimum": [0, 0, 0],
                "axis_maximum": [300, 300, 300]
            }),
        );
        let (_, markers) = surface.render(23., 37.);
        let labels = markers
            .iter()
            .map(|marker| marker.label.as_str())
            .collect::<Vec<_>>();
        assert!(labels.iter().any(|label| label.contains("X idx 1")));
        assert!(labels.contains(&"Y idx 0"));
        assert!(!labels.iter().any(|label| label.starts_with("X 300")));
    }
}
