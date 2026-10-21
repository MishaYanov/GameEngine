use renderer::vulkan::Vertex;

pub struct MeshData {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl MeshData {
    pub fn plane() -> Self {
        super::primitives::plane::build()
    }
    
    pub fn polygon(sides: u32) -> Self {
        super::primitives::polygon::build(sides)
    }

    pub fn sphere() -> Self {
        super::primitives::sphere::build()
    }

    pub fn capsule() -> Self {
        super::primitives::capsule::build()
    }

    pub fn triangle() -> Self {
        super::primitives::triangle::build()
    }
    pub fn new(vertices: Vec<Vertex>, indices: Vec<u32>) -> Self {
        Self { vertices, indices }
    }

    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn cube() -> Self {
        super::primitives::cube::build()
    }
}
