//! Typed graph values transcribed from the AutoCAD 2027 issue-135 oracle.
//! R2010, R2013, and R2018 share these 13 graphs; only the action version differs.

use acadrust::objects::{
    Assoc2dConstraintGroup, AssocAction, AssocConstraintNode, AssocConstraintNodeData as Data,
    AssociativeData, AssociativeObject, ObjectType,
};
use acadrust::{CadDocument, DxfVersion, Handle, Vector3};

fn xy(x: f64, y: f64) -> Vector3 {
    Vector3::new(x, y, 0.0)
}

fn node(id: i32, class: &str, status: u8, connections: &[i32], data: Data) -> AssocConstraintNode {
    AssocConstraintNode {
        node_id: id,
        class_name: class.into(),
        status,
        connections: connections.into(),
        data,
        ..Default::default()
    }
}

fn implicit(
    id: i32,
    status: u8,
    point_type: u8,
    point_index: i32,
    curve_id: i32,
    connections: &[i32],
) -> AssocConstraintNode {
    node(
        id,
        "AcConstrainedImplicitPoint",
        status,
        connections,
        Data::ImplicitPoint {
            geometry_dependency: Handle::NULL,
            geometry_node_id: 0,
            point: None,
            point_type,
            point_index,
            curve_id,
        },
    )
}

fn constraint(id: i32, class: &str, implied: bool, connections: &[i32]) -> AssocConstraintNode {
    node(
        id,
        class,
        0,
        connections,
        Data::Geometrical {
            owner_id: 0,
            is_implied: implied,
            is_active: true,
        },
    )
}

fn end_points(curve_id: i32, first: i32) -> Vec<AssocConstraintNode> {
    vec![
        implicit(first, 1, 0, -1, curve_id, &[first + 1]),
        constraint(
            first + 1,
            "AcPointCurveConstraint",
            true,
            &[first, curve_id],
        ),
        implicit(first + 2, 1, 1, -1, curve_id, &[first + 3]),
        constraint(
            first + 3,
            "AcPointCurveConstraint",
            true,
            &[first + 2, curve_id],
        ),
    ]
}

fn center_point(curve_id: i32, first: i32, status: u8) -> Vec<AssocConstraintNode> {
    vec![
        implicit(first, status, 3, -1, curve_id, &[first + 1]),
        constraint(
            first + 1,
            "AcCenterPointConstraint",
            true,
            &[first, curve_id],
        ),
    ]
}

fn line(
    id: i32,
    dependency: u64,
    start: Vector3,
    end: Vector3,
    direction: Vector3,
    explicit_constraint: i32,
) -> Vec<AssocConstraintNode> {
    let mut nodes = vec![node(
        id,
        "AcConstrainedBoundedLine",
        1,
        &[id + 2, id + 4, explicit_constraint],
        Data::BoundedLine {
            geometry_dependency: Handle::new(dependency),
            geometry_node_id: 0,
            point: start,
            direction,
            is_ray: false,
            start_point: start,
            end_point: end,
        },
    )];
    nodes.extend(end_points(id, id + 1));
    nodes
}

fn circle(
    id: i32,
    dependency: u64,
    center: Vector3,
    radius: f64,
    explicit_constraint: i32,
) -> Vec<AssocConstraintNode> {
    let mut nodes = vec![node(
        id,
        "AcConstrainedCircle",
        1,
        &[id + 2, explicit_constraint],
        Data::Circle {
            geometry_dependency: Handle::new(dependency),
            geometry_node_id: 0,
            center,
            normal: Vector3::UNIT_Z,
            direction: Vector3::UNIT_X,
            radius,
            start_parameter: 0.0,
            end_parameter: std::f64::consts::TAU,
            reserved: 0.0,
        },
    )];
    nodes.extend(center_point(id, id + 1, 1));
    nodes
}

fn axis(vertical: bool) -> Vec<AssocConstraintNode> {
    let (dependency, start, end, direction, datum, class) = if vertical {
        (
            0x2E0,
            xy(30.0, 0.0),
            xy(30.0, 20.0),
            Vector3::UNIT_Y,
            7,
            "AcVerticalConstraint",
        )
    } else {
        (
            0x2DD,
            Vector3::ZERO,
            xy(20.0, 0.0),
            Vector3::UNIT_X,
            6,
            "AcHorizontalConstraint",
        )
    };
    let mut nodes = line(1, dependency, start, end, direction, 8);
    nodes.push(node(
        datum,
        "AcConstrainedDatumLine",
        0,
        &[],
        Data::Line {
            geometry_dependency: Handle::NULL,
            geometry_node_id: 0,
            point: Vector3::ZERO,
            direction,
        },
    ));
    nodes.push(node(
        8,
        class,
        0,
        &[1],
        Data::Parallel {
            owner_id: 0,
            is_implied: false,
            is_active: true,
            datum_line_index: Some(datum),
        },
    ));
    nodes
}

fn line_pair(
    handle: u64,
    class: &str,
    a: [Vector3; 2],
    b: [Vector3; 2],
) -> Vec<AssocConstraintNode> {
    let mut nodes = line(1, handle + 2, a[0], a[1], Vector3::UNIT_X, 11);
    let direction = if b[0].x == b[1].x {
        Vector3::UNIT_Y
    } else {
        Vector3::UNIT_X
    };
    nodes.extend(line(6, handle + 4, b[0], b[1], direction, 11));
    nodes.push(if class == "AcParallelConstraint" {
        node(
            11,
            class,
            0,
            &[1, 6],
            Data::Parallel {
                owner_id: 0,
                is_implied: false,
                is_active: true,
                datum_line_index: None,
            },
        )
    } else {
        constraint(11, class, false, &[1, 6])
    });
    nodes
}

fn circle_pair(handle: u64, concentric: bool) -> Vec<AssocConstraintNode> {
    let (a, b, radius, class) = if concentric {
        (xy(0.0, 50.0), xy(0.0, 50.0), 15.0, "AcConcentricConstraint")
    } else {
        (
            xy(40.0, 50.0),
            xy(70.0, 50.0),
            10.0,
            "AcEqualRadiusConstraint",
        )
    };
    let mut nodes = circle(1, handle + 2, a, 10.0, 7);
    nodes.extend(circle(4, handle + 4, b, radius, 7));
    nodes.push(constraint(7, class, false, &[1, 4]));
    nodes
}

fn tangent() -> Vec<AssocConstraintNode> {
    let mut nodes = circle(1, 0x301, xy(100.0, 50.0), 10.0, 9);
    nodes.extend(line(
        4,
        0x303,
        xy(80.0, 40.0),
        xy(120.0, 40.0),
        Vector3::UNIT_X,
        9,
    ));
    nodes.push(constraint(9, "AcTangentConstraint", false, &[1, 4]));
    nodes
}

fn fixed_arc() -> Vec<AssocConstraintNode> {
    let mut nodes = vec![node(
        1,
        "AcConstrainedArc",
        0,
        &[3, 5, 7, 8],
        Data::Arc {
            geometry_dependency: Handle::new(0x306),
            geometry_node_id: 0,
            center: xy(140.0, 50.0),
            normal: Vector3::UNIT_Z,
            direction: Vector3::UNIT_X,
            radius: 10.0,
            start_parameter: 0.0,
            end_parameter: std::f64::consts::TAU,
            reserved: 0.0,
            start_point: xy(150.0, 50.0),
            end_point: xy(130.0, 50.0),
        },
    )];
    nodes.extend(end_points(1, 2));
    nodes.extend(center_point(1, 6, 0));
    nodes.push(constraint(8, "AcFixedConstraint", false, &[1]));
    nodes
}

fn fixed_ellipse(bounded: bool) -> Vec<AssocConstraintNode> {
    let mut nodes = if bounded {
        vec![node(
            1,
            "AcConstrainedBoundedEllipse",
            0,
            &[3, 5, 7, 8],
            Data::BoundedEllipse {
                geometry_dependency: Handle::new(0x30C),
                geometry_node_id: 0,
                center: xy(40.0, 100.0),
                major_axis: xy(15.0, 0.0),
                axis_ratio: 0.5,
                start_point: xy(55.0, 100.0),
                end_point: xy(25.0, 100.0),
            },
        )]
    } else {
        vec![node(
            1,
            "AcConstrainedEllipse",
            0,
            &[3, 4],
            Data::Ellipse {
                geometry_dependency: Handle::new(0x309),
                geometry_node_id: 0,
                center: xy(0.0, 100.0),
                major_axis: xy(15.0, 0.0),
                axis_ratio: 0.5,
            },
        )]
    };
    if bounded {
        nodes.extend(end_points(1, 2));
        nodes.extend(center_point(1, 6, 0));
        nodes.push(constraint(8, "AcFixedConstraint", false, &[1]));
    } else {
        nodes.extend(center_point(1, 2, 0));
        nodes.push(constraint(4, "AcFixedConstraint", false, &[1]));
    }
    nodes
}

fn fixed_spline() -> Vec<AssocConstraintNode> {
    let last_knot = 43.35023188854984;
    let mut nodes = vec![node(
        1,
        "AcConstrainedSpline",
        0,
        &[8],
        Data::Spline {
            geometry_dependency: Handle::new(0x30F),
            geometry_node_id: 0,
            rational: false,
            periodic: false,
            degree: 3,
            knot_tolerance: 1.0e-9,
            knot_physical_length: 10,
            knot_grow_length: 8,
            knots: vec![
                0.0,
                0.0,
                0.0,
                0.0,
                14.142135623730951,
                32.1698920010509,
                last_knot,
                last_knot,
                last_knot,
                last_knot,
            ],
            weight_physical_length: 0,
            weight_grow_length: 8,
            weights: vec![],
            control_point_physical_length: 8,
            control_point_grow_length: 8,
            control_points: vec![
                xy(70.0, 100.0),
                xy(73.62511708442378, 105.52690530485273),
                xy(81.87136985698146, 118.09926010951139),
                xy(86.8798263654781, 89.15459728636583),
                xy(96.36806931410393, 96.99777213215337),
                xy(100.0, 100.0),
            ],
            implicit_point_ids: vec![2, 3, 4, 5, 6, 7],
        },
    )];
    nodes.extend((0..6).map(|index| implicit(index + 2, 0, 4, index, 1, &[])));
    nodes.push(constraint(8, "AcFixedConstraint", false, &[1]));
    nodes
}

/// The complete native constraint graphs, independent of any drawing file or codec.
pub fn groups(version: DxfVersion) -> Vec<(Handle, Assoc2dConstraintGroup)> {
    let scenes = [
        (0x2DB, 8, vec![0x2DD], axis(false)),
        (0x2DE, 8, vec![0x2E0], axis(true)),
        (
            0x2E1,
            11,
            vec![0x2E5, 0x2E3],
            line_pair(
                0x2E1,
                "AcParallelConstraint",
                [xy(40.0, 0.0), xy(60.0, 0.0)],
                [xy(40.0, 10.0), xy(60.0, 10.0)],
            ),
        ),
        (
            0x2E6,
            11,
            vec![0x2EA, 0x2E8],
            line_pair(
                0x2E6,
                "AcPerpendicularConstraint",
                [xy(70.0, 0.0), xy(90.0, 0.0)],
                [xy(80.0, -10.0), xy(80.0, 10.0)],
            ),
        ),
        (
            0x2EB,
            11,
            vec![0x2EF, 0x2ED],
            line_pair(
                0x2EB,
                "AcColinearConstraint",
                [xy(100.0, 0.0), xy(110.0, 0.0)],
                [xy(120.0, 0.0), xy(130.0, 0.0)],
            ),
        ),
        (
            0x2F0,
            11,
            vec![0x2F4, 0x2F2],
            line_pair(
                0x2F0,
                "AcEqualLengthConstraint",
                [xy(140.0, 0.0), xy(160.0, 0.0)],
                [xy(140.0, 10.0), xy(160.0, 10.0)],
            ),
        ),
        (0x2F5, 7, vec![0x2F9, 0x2F7], circle_pair(0x2F5, true)),
        (0x2FA, 7, vec![0x2FE, 0x2FC], circle_pair(0x2FA, false)),
        (0x2FF, 9, vec![0x303, 0x301], tangent()),
        (0x304, 8, vec![0x306], fixed_arc()),
        (0x307, 4, vec![0x309], fixed_ellipse(false)),
        (0x30A, 8, vec![0x30C], fixed_ellipse(true)),
        (0x30D, 8, vec![0x30F], fixed_spline()),
    ];
    scenes
        .into_iter()
        .enumerate()
        .map(|(index, (handle, next_node_id, actions, mut nodes))| {
            nodes.insert(0, AssocConstraintNode::default());
            let actions: Vec<_> = actions.into_iter().map(Handle::new).collect();
            (
                Handle::new(handle),
                Assoc2dConstraintGroup {
                    action: AssocAction {
                        class_version: if version < DxfVersion::AC1027 { 1 } else { 2 },
                        owning_network: Handle::new(0x2D8),
                        action_index: index as i32 + 1,
                        max_dependency_index: actions.len() as i32,
                        ..Default::default()
                    },
                    version: 2,
                    work_plane: [Vector3::ZERO, Vector3::UNIT_X, Vector3::UNIT_Y],
                    actions,
                    next_node_id,
                    nodes,
                    ..Default::default()
                },
            )
        })
        .collect()
}

pub fn document(version: DxfVersion) -> CadDocument {
    let mut document = CadDocument::with_version(version);
    for (handle, group) in groups(version) {
        document.objects.insert(
            handle,
            ObjectType::Associative(Box::new(AssociativeObject {
                handle,
                owner: document.header.named_objects_dict_handle,
                dxf_name: "ASSOC2DCONSTRAINTGROUP".into(),
                cpp_class_name: "AcDbAssoc2dConstraintGroup".into(),
                data: AssociativeData::ConstraintGroup(group),
                ..Default::default()
            })),
        );
    }
    document
}
