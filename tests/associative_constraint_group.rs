use std::io::Cursor;

use acadrust::objects::{
    Assoc2dConstraintGroup, AssocAction, AssocConstraintNode, AssocConstraintNodeData,
    AssociativeData, AssociativeObject, ObjectType,
};
use acadrust::types::Vector3;
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

#[test]
fn r2010_version_two_group_reads_inline_nodes_with_status_and_active_flags() {
    use acadrust::io::dwg::{
        dwg_reference_type::DwgReferenceType,
        dwg_stream_readers::{
            bit_reader::DwgBitReader, merged_reader::DwgMergedReader,
            object_reader::associative::read_associative_data,
        },
        dwg_stream_writers::bit_writer::DwgBitWriter,
        DwgVersion,
    };
    use acadrust::types::DxfVersion;
    let dxf = DxfVersion::AC1024;
    let version = DwgVersion::from_dxf_version(dxf).unwrap();
    let mut main = DwgBitWriter::new(version, dxf);
    let mut text = DwgBitWriter::new(version, dxf);
    let mut handles = DwgBitWriter::new(version, dxf);
    main.write_bit_short(2);
    main.write_bit_long(0); // action version/status
    for _ in 0..3 {
        main.write_bit_long(0);
    } // action index, max dependency, count
    for _ in 0..2 {
        main.write_bit_short(0);
        main.write_bit_long(0);
    } // owned params/values
    main.write_bit_long(2);
    main.write_bit(false);
    for point in [Vector3::ZERO, Vector3::UNIT_X, Vector3::UNIT_Y] {
        main.write_3bit_double(point);
    }
    main.write_bit_long(0); // actions
    main.write_bit_long(9); // next node ID is not a node count
    main.write_bit_long(2); // inline count; no serialized root or class registry
    text.write_variable_text("AcConstrainedBoundedLine");
    main.write_bit_long(1);
    main.write_byte(3);
    main.write_bit_long(1);
    main.write_bit_long(4);
    main.write_bit_long(0);
    main.write_3bit_double(Vector3::ZERO);
    main.write_3bit_double(Vector3::UNIT_X);
    main.write_bit(false);
    main.write_3bit_double(Vector3::ZERO);
    main.write_3bit_double(Vector3::UNIT_X);
    text.write_variable_text("AcFixedConstraint");
    main.write_bit_long(4);
    main.write_byte(7);
    main.write_bit_long(1);
    main.write_bit_long(1);
    main.write_bit_long(1);
    main.write_bit(false);
    main.write_bit(true);
    for _ in 0..4 {
        handles.write_handle(DwgReferenceType::SoftPointer, 0);
    }
    let mut reader = DwgMergedReader::from_readers(
        DwgBitReader::new(main.take_bytes(), version, dxf),
        Some(DwgBitReader::new(text.take_bytes(), version, dxf)),
        Some(DwgBitReader::new(handles.take_bytes(), version, dxf)),
        dxf,
    );
    let AssociativeData::ConstraintGroup(group) =
        read_associative_data(&mut reader, "ASSOC2DCONSTRAINTGROUP", version, dxf).unwrap()
    else {
        panic!()
    };
    reader.check_error().unwrap();
    assert_eq!(group.version, 2);
    assert_eq!(group.next_node_id, 9);
    assert_eq!(group.nodes.len(), 3);
    assert_eq!((group.nodes[1].node_id, group.nodes[1].status), (1, 3));
    assert_eq!((group.nodes[2].node_id, group.nodes[2].status), (4, 7));
    assert_eq!(group.nodes[1].connections, vec![4]);
    assert!(matches!(
        group.nodes[2].data,
        AssocConstraintNodeData::Geometrical {
            owner_id: 1,
            is_implied: false,
            is_active: true
        }
    ));
}

#[test]
fn dwg_constraint_group_counts_only_registered_nodes() {
    let mut document = CadDocument::new();
    let handle = document.allocate_handle();
    let owner = document.header.named_objects_dict_handle;
    document.objects.insert(
        handle,
        ObjectType::Associative(Box::new(AssociativeObject {
            handle,
            owner,
            dxf_name: "ASSOC2DCONSTRAINTGROUP".to_string(),
            cpp_class_name: "AcDbAssoc2dConstraintGroup".to_string(),
            data: AssociativeData::ConstraintGroup(Assoc2dConstraintGroup {
                action: AssocAction {
                    class_version: 2,
                    ..Default::default()
                },
                version: 2,
                nodes: vec![
                    AssocConstraintNode::default(),
                    AssocConstraintNode {
                        node_id: 1,
                        class_name: "AcFixedConstraint".to_string(),
                        data: AssocConstraintNodeData::Geometrical {
                            owner_id: 0,
                            is_implied: false,
                            is_active: true,
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        })),
    );

    let bytes = DwgWriter::write_to_vec(&document).expect("write DWG");
    let decoded = DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .expect("read DWG");
    let group = decoded
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Associative(object) => match &object.data {
                AssociativeData::ConstraintGroup(group) => Some(group),
                _ => None,
            },
            _ => None,
        })
        .expect("constraint group should round-trip");

    assert_eq!(group.nodes.len(), 2);
    assert_eq!(group.nodes[0].node_id, 0);
    assert_eq!(group.nodes[1].node_id, 1);
    assert_eq!(group.nodes[1].class_name, "AcFixedConstraint");
}

fn document_with_axis_and_rigid_set() -> CadDocument {
    let mut document = CadDocument::new();
    let handle = document.allocate_handle();
    let owner = document.header.named_objects_dict_handle;
    let mut transform = [0.0; 16];
    transform[0] = 0.5;
    transform[5] = 0.5;
    transform[10] = 0.5;
    transform[15] = 1.0;
    transform[3] = 10.25;
    transform[7] = -2.5;
    document.objects.insert(
        handle,
        ObjectType::Associative(Box::new(AssociativeObject {
            handle,
            owner,
            dxf_name: "ASSOC2DCONSTRAINTGROUP".to_string(),
            cpp_class_name: "AcDbAssoc2dConstraintGroup".to_string(),
            data: AssociativeData::ConstraintGroup(Assoc2dConstraintGroup {
                action: AssocAction {
                    class_version: 2,
                    ..Default::default()
                },
                version: 2,
                nodes: vec![
                    AssocConstraintNode::default(),
                    AssocConstraintNode {
                        node_id: 1,
                        class_name: "AcConstrainedDatumLine".to_string(),
                        data: AssocConstraintNodeData::Line {
                            geometry_dependency: Default::default(),
                            geometry_node_id: 0,
                            point: Vector3::ZERO,
                            direction: Vector3::new(1.0, 0.0, 0.0),
                        },
                        ..Default::default()
                    },
                    AssocConstraintNode {
                        node_id: 2,
                        class_name: "AcHorizontalConstraint".to_string(),
                        data: AssocConstraintNodeData::Parallel {
                            owner_id: 0,
                            is_implied: false,
                            is_active: true,
                            datum_line_index: Some(1),
                        },
                        ..Default::default()
                    },
                    AssocConstraintNode {
                        node_id: 3,
                        class_name: "AcConstrainedRigidSet".to_string(),
                        data: AssocConstraintNodeData::RigidSet {
                            geometry_dependency: Default::default(),
                            geometry_node_id: 0,
                            reserved: false,
                            transform,
                            geometry_ids: vec![4, 5, 6],
                        },
                        ..Default::default()
                    },
                    AssocConstraintNode {
                        node_id: 4,
                        class_name: "AcConstrainedEllipse".to_string(),
                        data: AssocConstraintNodeData::Ellipse {
                            geometry_dependency: Default::default(),
                            geometry_node_id: 4,
                            center: Vector3::new(1.0, 2.0, 0.0),
                            major_axis: Vector3::new(4.0, 0.0, 0.0),
                            axis_ratio: 0.5,
                        },
                        ..Default::default()
                    },
                    AssocConstraintNode {
                        node_id: 5,
                        class_name: "AcConstrainedSpline".to_string(),
                        data: AssocConstraintNodeData::Spline {
                            geometry_dependency: Default::default(),
                            geometry_node_id: 5,
                            rational: false,
                            periodic: false,
                            degree: 2,
                            knot_tolerance: 1.0e-10,
                            knot_physical_length: 6,
                            knot_grow_length: 8,
                            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                            weight_physical_length: 0,
                            weight_grow_length: 8,
                            weights: Vec::new(),
                            control_point_physical_length: 3,
                            control_point_grow_length: 8,
                            control_points: vec![
                                Vector3::ZERO,
                                Vector3::new(1.0, 2.0, 0.0),
                                Vector3::new(3.0, 0.0, 0.0),
                            ],
                            implicit_point_ids: vec![10, 11, 12],
                        },
                        ..Default::default()
                    },
                    AssocConstraintNode {
                        node_id: 6,
                        class_name: "AcHelpParameter".to_string(),
                        data: AssocConstraintNodeData::HelpParameter {
                            value: 0.25,
                            reserved: true,
                        },
                        ..Default::default()
                    },
                    AssocConstraintNode {
                        node_id: 7,
                        class_name: "AcG2SmoothConstraint".to_string(),
                        data: AssocConstraintNodeData::Composite {
                            owner_id: 5,
                            is_implied: false,
                            is_active: true,
                            owned_constraint_ids: vec![8, 9],
                        },
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        })),
    );
    document
}

fn assert_axis_and_rigid_set(document: &CadDocument) {
    let group = document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Associative(object) => match &object.data {
                AssociativeData::ConstraintGroup(group) => Some(group),
                _ => None,
            },
            _ => None,
        })
        .expect("constraint group should round-trip");
    assert!(matches!(
        group.nodes[2].data,
        AssocConstraintNodeData::Parallel {
            datum_line_index: Some(1),
            ..
        }
    ));
    assert!(matches!(
        &group.nodes[3].data,
        AssocConstraintNodeData::RigidSet {
            transform,
            geometry_ids,
            ..
        } if transform[0] == 0.5
            && transform[3] == 10.25
            && geometry_ids == &[4, 5, 6]
    ));
    assert!(matches!(
        group.nodes[4].data,
        AssocConstraintNodeData::Ellipse {
            geometry_node_id: 4,
            major_axis,
            axis_ratio: 0.5,
            ..
        } if major_axis == Vector3::new(4.0, 0.0, 0.0)
    ));
    assert!(matches!(
        &group.nodes[5].data,
        AssocConstraintNodeData::Spline {
            geometry_node_id: 5,
            rational: false,
            degree: 2,
            knots,
            control_points,
            implicit_point_ids,
            ..
        } if knots.len() == 6
            && control_points[1] == Vector3::new(1.0, 2.0, 0.0)
            && implicit_point_ids == &[10, 11, 12]
    ));
    assert!(matches!(
        group.nodes[6].data,
        AssocConstraintNodeData::HelpParameter {
            value: 0.25,
            reserved: true,
        }
    ));
    assert!(matches!(
        &group.nodes[7].data,
        AssocConstraintNodeData::Composite {
            owner_id: 5,
            owned_constraint_ids,
            ..
        } if owned_constraint_ids == &[8, 9]
    ));
}

#[test]
fn axis_constraint_and_rigid_set_round_trip_in_dwg_and_dxf() {
    let document = document_with_axis_and_rigid_set();
    let dwg = DwgReader::from_stream(Cursor::new(
        DwgWriter::write_to_vec(&document).expect("write DWG"),
    ))
    .read()
    .expect("read DWG");
    assert_axis_and_rigid_set(&dwg);

    let dxf = DxfReader::from_reader(Cursor::new(
        DxfWriter::new(&document).write_to_vec().expect("write DXF"),
    ))
    .expect("create DXF reader")
    .read()
    .expect("read DXF");
    assert_axis_and_rigid_set(&dxf);
}
