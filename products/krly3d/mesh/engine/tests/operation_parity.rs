#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_kernel::model::Statement;
use krly3d_kernel::schema::Params;
use krly3d_mesh::operation::{encode_event, MeshEventBody, OperationCall};
use krly3d_mesh::{MeshCall, MeshInput, RequestId};
use krly3d_mesh_engine::runner::{MeshRunner, RunnerLimits};
use krly3d_mesh_engine::MeshEngine;
use krlyxd::SystemClock;
use std::sync::Arc;

#[test]
fn operation_results_preserve_the_synchronous_contract_and_quality() {
    let mut kernel = krly3d_std::standard().build();
    kernel
        .apply(
            Statement::new("part", "box", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0), &[])
                .unwrap(),
        )
        .unwrap();
    let engine = MeshEngine::new(kernel).unwrap();
    let runner = MeshRunner::new(RunnerLimits::default(), Arc::new(SystemClock::default())).unwrap();
    for (ordinal, (node, triangles, tolerance)) in
        [("part", false, 0.05), ("part@1", true, 0.05), ("part", true, 0.01)].into_iter().enumerate()
    {
        let mut input = MeshInput::new(node);
        input.triangles = triangles;
        input.tolerance.value = tolerance;
        let request_id = RequestId::new(&format!("parity-{ordinal}"));
        let expected = engine.call(MeshCall::new(request_id.clone(), input.clone())).unwrap();
        let id = runner.start(&engine, OperationCall::new(request_id.clone(), input)).unwrap();
        let first = runner.wait_next(&id).unwrap().unwrap();
        assert_eq!((&first.request_id, &first.operation_id, first.seq), (&request_id, &id, 0));
        assert!(encode_event(&first).is_ok());
        assert_eq!(first.body, MeshEventBody::Started(expected.provenance.clone()));
        let terminal = runner.wait_next(&id).unwrap().unwrap();
        assert_eq!((&terminal.request_id, &terminal.operation_id, terminal.seq), (&request_id, &id, 1));
        assert!(encode_event(&terminal).is_ok());
        let MeshEventBody::Completed(mut actual) = terminal.body else { panic!("mesh did not complete") };
        assert_eq!((actual.volume, actual.area, actual.open), (48.0, 88.0, 0));
        if let (Some(actual), Some(expected)) = (&mut actual.mesh, &expected.mesh) {
            // Separate operations have separate artifact IDs; all other fields must match.
            assert_ne!(actual.id, expected.id);
            actual.id = expected.id.clone();
        }
        assert_eq!(actual, expected);
        assert_eq!(runner.wait_next(&id).unwrap(), None);
        runner.cancel(&id).unwrap();
        assert_eq!(runner.next(&id).unwrap(), None);
    }
}
