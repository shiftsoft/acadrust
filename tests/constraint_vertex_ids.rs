use acadrust::entities::{EntityType, LwPolyline, LwVertex};
use acadrust::objects::{
    AssocDependency, AssocGeomDependency, AssocPersistentSubentId, AssociativeData,
    AssociativeObject, ObjectType,
};
use acadrust::types::{DxfVersion, Handle};
use acadrust::{CadDocument, DxfWriter};

fn constrained_polyline(document: &mut CadDocument, vertices: usize, closed: bool) -> Handle {
    let mut polyline = LwPolyline::new();
    polyline.is_closed = closed;
    for index in 0..vertices {
        polyline
            .vertices
            .push(LwVertex::from_coords(index as f64 * 10.0, 0.0));
    }
    document
        .add_entity(EntityType::LwPolyline(polyline))
        .expect("add polyline")
}

fn add_edge_dependency(document: &mut CadDocument, host: Handle, from: i32, to: i32) {
    let handle = document.allocate_handle();
    let owner = document.header.named_objects_dict_handle;
    document.objects.insert(
        handle,
        ObjectType::Associative(Box::new(AssociativeObject {
            handle,
            owner,
            dxf_name: "ACDBASSOCGEOMDEPENDENCY".to_string(),
            cpp_class_name: "AcDbAssocGeomDependency".to_string(),
            data: AssociativeData::GeomDependency(AssocGeomDependency {
                dependency: AssocDependency {
                    dependent_on: host,
                    ..Default::default()
                },
                persistent_subent: AssocPersistentSubentId {
                    class_name: "AcDbAssocEdgePersSubentId".to_string(),
                    values: vec![0, from, to],
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        })),
    );
}

fn vertex_ids(document: &CadDocument, host: Handle) -> Vec<i32> {
    document
        .entities()
        .find_map(|entity| match entity {
            EntityType::LwPolyline(polyline) if polyline.common.handle == host => Some(
                polyline
                    .vertices
                    .iter()
                    .map(|vertex| vertex.vertex_id)
                    .collect(),
            ),
            _ => None,
        })
        .expect("polyline should exist")
}

#[test]
fn assigns_vertex_ids_along_closed_constraint_edge_chain() {
    let mut document = CadDocument::new();
    let host = constrained_polyline(&mut document, 4, true);
    for (from, to) in [(60, 61), (61, 62), (62, 63), (63, 60)] {
        add_edge_dependency(&mut document, host, from, to);
    }

    let assigned = document.restore_constraint_vertex_ids();

    assert_eq!(assigned, 1);
    assert_eq!(vertex_ids(&document, host), vec![60, 61, 62, 63]);
}

#[test]
fn assigns_vertex_ids_along_open_constraint_edge_chain() {
    let mut document = CadDocument::new();
    let host = constrained_polyline(&mut document, 3, false);
    for (from, to) in [(10, 11), (11, 12)] {
        add_edge_dependency(&mut document, host, from, to);
    }

    let assigned = document.restore_constraint_vertex_ids();

    assert_eq!(assigned, 1);
    assert_eq!(vertex_ids(&document, host), vec![10, 11, 12]);
}

#[test]
fn keeps_existing_vertex_ids() {
    let mut document = CadDocument::new();
    let host = constrained_polyline(&mut document, 4, true);
    for entity in document.entities_mut() {
        let EntityType::LwPolyline(polyline) = entity else {
            continue;
        };
        if polyline.common.handle != host {
            continue;
        }
        for (vertex, id) in polyline.vertices.iter_mut().zip([5, 6, 7, 8]) {
            vertex.vertex_id = id;
        }
    }
    for (from, to) in [(60, 61), (61, 62), (62, 63), (63, 60)] {
        add_edge_dependency(&mut document, host, from, to);
    }

    let assigned = document.restore_constraint_vertex_ids();

    assert_eq!(assigned, 0);
    assert_eq!(vertex_ids(&document, host), vec![5, 6, 7, 8]);
}

#[test]
fn skips_polylines_without_constraint_edges() {
    let mut document = CadDocument::new();
    let host = constrained_polyline(&mut document, 4, true);

    let assigned = document.restore_constraint_vertex_ids();

    assert_eq!(assigned, 0);
    assert_eq!(vertex_ids(&document, host), vec![0, 0, 0, 0]);
}

#[test]
fn skips_incomplete_constraint_edge_chains() {
    let mut document = CadDocument::new();
    let host = constrained_polyline(&mut document, 4, true);
    for (from, to) in [(60, 61), (61, 60)] {
        add_edge_dependency(&mut document, host, from, to);
    }

    let assigned = document.restore_constraint_vertex_ids();

    assert_eq!(assigned, 0);
    assert_eq!(vertex_ids(&document, host), vec![0, 0, 0, 0]);
}

#[test]
fn ignores_non_edge_subentity_dependencies() {
    let mut document = CadDocument::new();
    let host = constrained_polyline(&mut document, 4, true);
    let handle = document.allocate_handle();
    let owner = document.header.named_objects_dict_handle;
    document.objects.insert(
        handle,
        ObjectType::Associative(Box::new(AssociativeObject {
            handle,
            owner,
            dxf_name: "ACDBASSOCGEOMDEPENDENCY".to_string(),
            cpp_class_name: "AcDbAssocGeomDependency".to_string(),
            data: AssociativeData::GeomDependency(AssocGeomDependency {
                dependency: AssocDependency {
                    dependent_on: host,
                    ..Default::default()
                },
                persistent_subent: AssocPersistentSubentId {
                    class_name: "AcDbAssocVertexPersSubentId".to_string(),
                    values: vec![0, 60],
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        })),
    );

    let assigned = document.restore_constraint_vertex_ids();

    assert_eq!(assigned, 0);
    assert_eq!(vertex_ids(&document, host), vec![0, 0, 0, 0]);
}

fn dxf_pairs(text: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .chunks_exact(2)
        .map(|chunk| {
            (
                chunk[0].trim().parse().expect("group code"),
                chunk[1].trim().to_string(),
            )
        })
        .collect()
}

fn dxf_entity_codes(pairs: &[(i32, String)], entity: &str) -> Vec<(i32, String)> {
    let mut inside = false;
    let mut codes = Vec::new();
    for pair in pairs {
        if pair.0 == 0 {
            inside = pair.1 == entity;
            continue;
        }
        if inside {
            codes.push(pair.clone());
        }
    }
    codes
}

#[test]
fn dxf_vertex_ids_written_only_for_r2010_and_newer() {
    let mut document = CadDocument::with_version(DxfVersion::AC1015);
    let host = constrained_polyline(&mut document, 4, true);
    for entity in document.entities_mut() {
        let EntityType::LwPolyline(polyline) = entity else {
            continue;
        };
        if polyline.common.handle != host {
            continue;
        }
        for (vertex, id) in polyline.vertices.iter_mut().zip([60, 61, 62, 63]) {
            vertex.vertex_id = id;
        }
    }

    let write = |document: &CadDocument| -> Vec<(i32, String)> {
        let text = String::from_utf8(DxfWriter::new(document).write_to_vec().expect("write DXF"))
            .expect("utf-8");
        dxf_entity_codes(&dxf_pairs(&text), "LWPOLYLINE")
    };

    document.version = DxfVersion::AC1015;
    let codes = write(&document);
    assert!(
        !codes.iter().any(|(code, _)| *code == 91),
        "pre-R2010 DXF must not carry vertex ids: {codes:?}"
    );

    document.version = DxfVersion::AC1024;
    let codes = write(&document);
    assert!(
        codes.contains(&(91, "60".to_string())),
        "R2010+ DXF should carry vertex ids: {codes:?}"
    );
}
