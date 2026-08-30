use serde::Serialize;
use std::cmp::Ordering;

pub const TILES: usize = 32;
pub const VERTEX_COUNT: usize = (TILES + 1) * (TILES + 1);
pub const TRIANGLE_COUNT: usize = TILES * TILES * 2;
pub const INDEX_COUNT: usize = TRIANGLE_COUNT * 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub shade: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct MeshVertex {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub shade: f32,
}

#[derive(Clone, Copy, Debug)]
struct Triangle {
    indices: [u32; 3],
    depth: f32,
}

fn project_vertex(vertex: &mut Vertex, width: f32, height: f32) {
    let focal_length = width.max(height) * 2.0;
    let scale = focal_length / (focal_length - vertex.z).max(1.0);

    vertex.x = width / 2.0 + (vertex.x - width / 2.0) * scale;
    vertex.y = height / 2.0 + (vertex.y - height / 2.0) * scale;
}

fn deform_vertex_with_rotation(
    width: f32,
    height: f32,
    period: f64,
    cosine: f32,
    sine: f32,
    radius: f32,
    vertex: &mut Vertex,
) {
    let cx = (1.0 - period) as f32 * width;
    let cy = (1.0 - period) as f32 * height;
    let dx = vertex.x - cx;
    let dy = vertex.y - cy;
    let mut rx = dx * cosine + dy * sine - radius;
    let ry = -dx * sine + dy * cosine;
    let mut turn_angle = 0.0_f32;

    vertex.z = 0.0;
    vertex.shade = 1.0;
    if rx > radius * -2.0 {
        turn_angle = rx / radius * std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_2;
        vertex.shade = (turn_angle.sin() * 96.0 + 159.0) / 255.0;
    }

    if rx > 0.0 {
        let small_radius = radius - radius.min(turn_angle * 10.0 / std::f32::consts::PI);
        rx = small_radius * turn_angle.cos() + radius;
        vertex.x = rx * cosine - ry * sine + cx;
        vertex.y = rx * sine + ry * cosine + cy;
        vertex.z = small_radius * turn_angle.sin() + radius;
    }
}

pub fn deform_vertex(
    width: f32,
    height: f32,
    period: f64,
    angle: f64,
    radius: f32,
    vertex: &mut Vertex,
) {
    assert!((0.0..=1.0).contains(&period));
    assert!(radius > 0.0);

    vertex.z = 0.0;
    vertex.shade = 1.0;
    if period == 0.0 {
        return;
    }

    let radians = (angle * (std::f64::consts::PI / 180.0)) as f32;
    deform_vertex_with_rotation(
        width,
        height,
        period,
        radians.cos(),
        radians.sin(),
        radius,
        vertex,
    );
}

pub fn build_mesh(width: f32, height: f32, period: f64, angle: f64) -> (Vec<MeshVertex>, Vec<u32>) {
    build_mesh_with_tiles(width, height, period, angle, TILES)
}

pub fn build_mesh_with_tiles(
    width: f32,
    height: f32,
    period: f64,
    angle: f64,
    tiles: usize,
) -> (Vec<MeshVertex>, Vec<u32>) {
    assert!(width > 0.0);
    assert!(height > 0.0);
    assert!((0.0..=1.0).contains(&period));
    assert!(tiles > 0);

    let vertex_count = (tiles + 1) * (tiles + 1);
    let triangle_count = tiles * tiles * 2;
    let index_count = triangle_count * 3;
    let mut vertices = vec![MeshVertex::default(); vertex_count];
    let mut depths = vec![0.0_f32; vertex_count];
    let radians = (angle * (std::f64::consts::PI / 180.0)) as f32;
    let cosine = radians.cos();
    let sine = radians.sin();

    for y in 0..=tiles {
        for x in 0..=tiles {
            let vertex_index = y * (tiles + 1) + x;
            let source_x = width * x as f32 / tiles as f32;
            let source_y = height * y as f32 / tiles as f32;
            let mut destination = Vertex {
                x: source_x,
                y: source_y,
                shade: 1.0,
                ..Vertex::default()
            };

            if period > 0.0 {
                deform_vertex_with_rotation(
                    width,
                    height,
                    period,
                    cosine,
                    sine,
                    50.0,
                    &mut destination,
                );
                project_vertex(&mut destination, width, height);
            }
            depths[vertex_index] = destination.z;
            vertices[vertex_index] = MeshVertex {
                x: destination.x / width * 2.0 - 1.0,
                y: 1.0 - destination.y / height * 2.0,
                u: x as f32 / tiles as f32,
                v: y as f32 / tiles as f32,
                shade: destination.shade,
            };
        }
    }

    let mut indices = Vec::with_capacity(index_count);
    if period == 0.0 {
        for y in 0..tiles {
            for x in 0..tiles {
                let top_left = (y * (tiles + 1) + x) as u32;
                let top_right = top_left + 1;
                let bottom_left = top_left + tiles as u32 + 1;
                let bottom_right = bottom_left + 1;
                indices.extend_from_slice(&[
                    top_left,
                    top_right,
                    bottom_right,
                    top_left,
                    bottom_right,
                    bottom_left,
                ]);
            }
        }
        return (vertices, indices);
    }

    let mut triangles = Vec::with_capacity(triangle_count);
    for y in 0..tiles {
        for x in 0..tiles {
            let top_left = y * (tiles + 1) + x;
            let top_right = top_left + 1;
            let bottom_left = top_left + tiles + 1;
            let bottom_right = bottom_left + 1;
            triangles.push(Triangle {
                indices: [top_left as u32, top_right as u32, bottom_right as u32],
                depth: (depths[top_left] + depths[top_right] + depths[bottom_right]) / 3.0,
            });
            triangles.push(Triangle {
                indices: [top_left as u32, bottom_right as u32, bottom_left as u32],
                depth: (depths[top_left] + depths[bottom_right] + depths[bottom_left]) / 3.0,
            });
        }
    }
    triangles.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(Ordering::Equal));
    for triangle in triangles {
        indices.extend_from_slice(&triangle.indices);
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    #[test]
    fn mesh_contract() {
        let (vertices, indices) = build_mesh(1280.0, 720.0, 0.55, 25.0);
        assert_eq!(vertices.len(), VERTEX_COUNT);
        assert_eq!(indices.len(), INDEX_COUNT);
        assert!(vertices.iter().all(|vertex| {
            vertex.x.is_finite()
                && vertex.y.is_finite()
                && vertex.shade > 0.0
                && vertex.shade <= 1.0
        }));
        assert!(vertices.iter().any(|vertex| vertex.shade < 0.99));
        assert!(indices.iter().all(|index| *index < VERTEX_COUNT as u32));

        let (vertices, indices) = build_mesh(800.0, 600.0, 0.0, 0.0);
        assert!((vertices[0].x + 1.0).abs() <= 0.0001);
        assert!((vertices[0].y - 1.0).abs() <= 0.0001);
        assert!((vertices[VERTEX_COUNT - 1].x - 1.0).abs() <= 0.0001);
        assert!((vertices[VERTEX_COUNT - 1].y + 1.0).abs() <= 0.0001);
        assert_eq!(&indices[..3], &[0, 1, TILES as u32 + 2]);
    }

    #[test]
    fn mesh_density_is_selectable() {
        let (vertices, indices) = build_mesh_with_tiles(1280.0, 720.0, 0.55, 15.0, 12);
        assert_eq!(vertices.len(), 13 * 13);
        assert_eq!(indices.len(), 12 * 12 * 2 * 3);
        assert!(indices.iter().all(|index| *index < vertices.len() as u32));
    }

    #[test]
    fn release_cpu_measurement() {
        for index in 0..10 {
            black_box(build_mesh(
                1920.0,
                1080.0,
                (index as f64 + 1.0) / 11.0,
                25.0,
            ));
        }
        let started = Instant::now();
        let mut checksum = 0_u32;
        for frame in 0..600 {
            let (_, indices) = build_mesh(1920.0, 1080.0, ((frame % 59) as f64 + 1.0) / 60.0, 25.0);
            checksum = checksum.wrapping_add(indices[frame % INDEX_COUNT]);
        }
        let elapsed = started.elapsed();
        eprintln!(
            "600 curved meshes: {:.2} ms ({:.3} ms/frame), checksum {checksum}",
            elapsed.as_secs_f64() * 1000.0,
            elapsed.as_secs_f64() * 1000.0 / 600.0,
        );
        assert!(elapsed.as_millis() < 900);
    }

    #[test]
    fn time_bounded_mesh_throughput() {
        let target = Duration::from_millis(200);
        let started = Instant::now();
        let mut frames = 0_u64;
        let mut checksum = 0_u32;
        while started.elapsed() < target {
            let (_, indices) =
                build_mesh(1920.0, 1080.0, ((frames % 59) as f64 + 1.0) / 60.0, 25.0);
            checksum = checksum.wrapping_add(indices[frames as usize % INDEX_COUNT]);
            frames += 1;
        }
        let elapsed = started.elapsed();
        eprintln!(
            "dynamic mesh throughput: {:.0} meshes/s over {:.0} ms, checksum {checksum}",
            frames as f64 / elapsed.as_secs_f64(),
            elapsed.as_secs_f64() * 1000.0,
        );
        assert!(frames > 0);
    }
}
