#![allow(dead_code)]

use gsk::prelude::IsRenderNode;
use gtk::{graphene, gsk};
use pinpoint_core::page_curl::{MeshVertex, build_mesh_with_tiles};

const MIN_STRAIGHT_TILES: usize = 16;
const MIN_ANGLED_TILES: usize = 6;
const MAX_STRAIGHT_TILES: usize = 64;
const MAX_ANGLED_TILES: usize = 10;

fn density(width: f32, height: f32, scale: i32, angled: bool) -> usize {
    if let Ok(value) = std::env::var("PINPOINT_GSK_CURL_TILES")
        && let Ok(value) = value.parse::<usize>()
    {
        return value.clamp(1, MAX_STRAIGHT_TILES);
    }
    let longest = width.max(height) * scale.max(1) as f32;
    let target = if angled { 240.0 } else { 48.0 };
    let maximum = if angled {
        MAX_ANGLED_TILES
    } else {
        MAX_STRAIGHT_TILES
    };
    let minimum = if angled {
        MIN_ANGLED_TILES
    } else {
        MIN_STRAIGHT_TILES
    };
    ((longest / target).ceil() as usize).clamp(minimum, maximum)
}

fn destination(vertex: MeshVertex, width: f32, height: f32) -> graphene::Point {
    graphene::Point::new(
        (vertex.x + 1.0) * width * 0.5,
        (1.0 - vertex.y) * height * 0.5,
    )
}

fn source(vertex: MeshVertex, width: f32, height: f32) -> graphene::Point {
    graphene::Point::new(vertex.u * width, vertex.v * height)
}

fn affine_coefficients(
    source: [graphene::Point; 3],
    destination: [graphene::Point; 3],
) -> (f32, f32, f32, f32, f32, f32) {
    let sx1 = source[1].x() - source[0].x();
    let sy1 = source[1].y() - source[0].y();
    let sx2 = source[2].x() - source[0].x();
    let sy2 = source[2].y() - source[0].y();
    let dx1 = destination[1].x() - destination[0].x();
    let dy1 = destination[1].y() - destination[0].y();
    let dx2 = destination[2].x() - destination[0].x();
    let dy2 = destination[2].y() - destination[0].y();
    let determinant = sx1 * sy2 - sx2 * sy1;
    debug_assert!(determinant.abs() > f32::EPSILON);
    let xx = (dx1 * sy2 - dx2 * sy1) / determinant;
    let xy = (-dx1 * sx2 + dx2 * sx1) / determinant;
    let yx = (dy1 * sy2 - dy2 * sy1) / determinant;
    let yy = (-dy1 * sx2 + dy2 * sx1) / determinant;
    let tx = destination[0].x() - xx * source[0].x() - xy * source[0].y();
    let ty = destination[0].y() - yx * source[0].x() - yy * source[0].y();
    (xx, yx, xy, yy, tx, ty)
}

fn affine(source: [graphene::Point; 3], destination: [graphene::Point; 3]) -> gsk::Transform {
    let (xx, yx, xy, yy, tx, ty) = affine_coefficients(source, destination);
    gsk::Transform::new().matrix_2d(xx, yx, xy, yy, tx, ty)
}

fn triangle_path(points: [graphene::Point; 3]) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.move_to(points[0].x(), points[0].y());
    builder.line_to(points[1].x(), points[1].y());
    builder.line_to(points[2].x(), points[2].y());
    builder.close();
    builder.to_path()
}

fn shade_node(node: &gsk::RenderNode, shade: f32) -> gsk::RenderNode {
    if shade >= 0.999 {
        return node.clone();
    }
    let matrix = graphene::Matrix::from_float([
        shade, 0.0, 0.0, 0.0, 0.0, shade, 0.0, 0.0, 0.0, 0.0, shade, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    gsk::ColorMatrixNode::new(node, &matrix, &graphene::Vec4::new(0.0, 0.0, 0.0, 0.0)).upcast()
}

fn straight_indices(vertices: &[MeshVertex], indices: &[u32], tiles: usize) -> Vec<u32> {
    let row = tiles + 1;
    let mut columns = Vec::with_capacity(tiles);
    let mut seen = vec![false; tiles];
    for triangle in indices.as_chunks::<3>().0 {
        let column = triangle
            .iter()
            .map(|index| *index as usize % row)
            .min()
            .unwrap_or(0)
            .min(tiles - 1);
        if !seen[column] {
            seen[column] = true;
            columns.push(column);
        }
    }

    let mut result = Vec::with_capacity(tiles * 6);
    for column in columns {
        let top_left = column as u32;
        let top_right = top_left + 1;
        let bottom_left = (tiles * row + column) as u32;
        let bottom_right = bottom_left + 1;
        let first_shade = (vertices[top_left as usize].shade
            + vertices[top_right as usize].shade
            + vertices[bottom_right as usize].shade)
            / 3.0;
        let second_shade = (vertices[top_left as usize].shade
            + vertices[bottom_right as usize].shade
            + vertices[bottom_left as usize].shade)
            / 3.0;
        if first_shade <= second_shade {
            result.extend_from_slice(&[top_left, top_right, bottom_right]);
            result.extend_from_slice(&[top_left, bottom_right, bottom_left]);
        } else {
            result.extend_from_slice(&[top_left, bottom_right, bottom_left]);
            result.extend_from_slice(&[top_left, top_right, bottom_right]);
        }
    }
    result
}

pub fn node(
    slide: &gsk::RenderNode,
    width: f32,
    height: f32,
    output_scale: i32,
    period: f64,
    angle: f64,
) -> gsk::RenderNode {
    if period <= f64::EPSILON {
        return slide.clone();
    }

    let angled = angle.abs() > f64::EPSILON;
    let tiles = density(width, height, output_scale, angled);
    let (vertices, mesh_indices) = build_mesh_with_tiles(width, height, period, angle, tiles);
    let indices = if angled {
        mesh_indices
    } else {
        straight_indices(&vertices, &mesh_indices, tiles)
    };
    let mut children = Vec::with_capacity(indices.len() / 3);
    for triangle in indices.as_chunks::<3>().0 {
        let source_points = [
            source(vertices[triangle[0] as usize], width, height),
            source(vertices[triangle[1] as usize], width, height),
            source(vertices[triangle[2] as usize], width, height),
        ];
        let destination_points = [
            destination(vertices[triangle[0] as usize], width, height),
            destination(vertices[triangle[1] as usize], width, height),
            destination(vertices[triangle[2] as usize], width, height),
        ];
        let path = triangle_path(source_points);
        let clipped = gsk::FillNode::new(slide, &path, gsk::FillRule::Winding).upcast();
        let shade = triangle
            .iter()
            .map(|index| vertices[*index as usize].shade)
            .sum::<f32>()
            / 3.0;
        let shaded = shade_node(&clipped, shade);
        children.push(
            gsk::TransformNode::new(shaded, Some(&affine(source_points, destination_points)))
                .upcast(),
        );
    }
    gsk::ContainerNode::new(&children).upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_scales_but_is_bounded() {
        assert_eq!(density(640.0, 360.0, 1, false), MIN_STRAIGHT_TILES);
        assert!(density(1920.0, 1080.0, 1, false) > MIN_STRAIGHT_TILES);
        assert_eq!(density(7680.0, 4320.0, 2, false), MAX_STRAIGHT_TILES);
        assert_eq!(density(7680.0, 4320.0, 2, true), MAX_ANGLED_TILES);
    }

    #[test]
    fn affine_maps_three_points_exactly() {
        let source = [
            graphene::Point::new(10.0, 20.0),
            graphene::Point::new(30.0, 20.0),
            graphene::Point::new(10.0, 50.0),
        ];
        let destination = [
            graphene::Point::new(15.0, 25.0),
            graphene::Point::new(55.0, 30.0),
            graphene::Point::new(20.0, 70.0),
        ];
        let (xx, yx, xy, yy, tx, ty) = affine_coefficients(source, destination);
        for (from, to) in source.into_iter().zip(destination) {
            let mapped_x = xx * from.x() + xy * from.y() + tx;
            let mapped_y = yx * from.x() + yy * from.y() + ty;
            assert!((mapped_x - to.x()).abs() < 0.001);
            assert!((mapped_y - to.y()).abs() < 0.001);
        }
    }
}
