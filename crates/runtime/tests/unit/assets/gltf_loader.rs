pub(super) const MODEL: &str = r#"{
    "asset":{"version":"2.0"},
    "buffers":[{"byteLength":42}],
    "bufferViews":[
        {"buffer":0,"byteOffset":0,"byteLength":36},
        {"buffer":0,"byteOffset":36,"byteLength":6}
    ],
    "accessors":[
        {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3",
         "min":[0,0,0],"max":[1,1,0]},
        {"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}
    ],
    "materials":[
        {"doubleSided":true,"pbrMetallicRoughness":{"baseColorFactor":[1,0,0,1]}},
        {"pbrMetallicRoughness":{"baseColorFactor":[0,1,0,1]}}
    ],
    "meshes":[{"primitives":[
        {"attributes":{"POSITION":0},"indices":1,"material":0,"mode":4},
        {"attributes":{"POSITION":0},"indices":1,"material":1,"mode":4}
    ]}]
}"#;

pub(super) fn geometry() -> Vec<gltf::buffer::Data> {
	let mut bytes = Vec::new();
	for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
		bytes.extend_from_slice(&value.to_le_bytes());
	}
	for index in [0_u16, 1, 2] {
		bytes.extend_from_slice(&index.to_le_bytes());
	}
	vec![gltf::buffer::Data(bytes)]
}
