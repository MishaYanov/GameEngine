use renderer::vulkan::Vertex;

pub struct MeshData {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl MeshData {
    pub fn triangle() -> Self {
        Self::new(vec![
            Vertex::new([-0.7, -0.6, 0.0], [0.0, 1.0]),
            Vertex::new([0.7, -0.6, 0.0], [1.0, 1.0]),
            Vertex::new([0.0, 0.7, 0.0], [0.5, 0.0]),
        ], vec![0, 1, 2, 2, 1, 0])
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
        let vertices = vec![
            /*
             * Front +Z
             */
            Vertex::new([-0.5, -0.5, 0.5], [0.0, 0.0]),
            Vertex::new([0.5, -0.5, 0.5], [1.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.5], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, 0.5], [0.0, 1.0]),
            /*
             * Back -Z
             */
            Vertex::new([0.5, -0.5, -0.5], [0.0, 0.0]),
            Vertex::new([-0.5, -0.5, -0.5], [1.0, 0.0]),
            Vertex::new([-0.5, 0.5, -0.5], [1.0, 1.0]),
            Vertex::new([0.5, 0.5, -0.5], [0.0, 1.0]),
            /*
             * Left -X
             */
            Vertex::new([-0.5, -0.5, -0.5], [0.0, 0.0]),
            Vertex::new([-0.5, -0.5, 0.5], [1.0, 0.0]),
            Vertex::new([-0.5, 0.5, 0.5], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, -0.5], [0.0, 1.0]),
            /*
             * Right +X
             */
            Vertex::new([0.5, -0.5, 0.5], [0.0, 0.0]),
            Vertex::new([0.5, -0.5, -0.5], [1.0, 0.0]),
            Vertex::new([0.5, 0.5, -0.5], [1.0, 1.0]),
            Vertex::new([0.5, 0.5, 0.5], [0.0, 1.0]),
            /*
             * Top +Y
             */
            Vertex::new([-0.5, 0.5, 0.5], [0.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.5], [1.0, 0.0]),
            Vertex::new([0.5, 0.5, -0.5], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, -0.5], [0.0, 1.0]),
            /*
             * Bottom -Y
             */
            Vertex::new([-0.5, -0.5, -0.5], [0.0, 0.0]),
            Vertex::new([0.5, -0.5, -0.5], [1.0, 0.0]),
            Vertex::new([0.5, -0.5, 0.5], [1.0, 1.0]),
            Vertex::new([-0.5, -0.5, 0.5], [0.0, 1.0]),
        ];

        let indices = vec![
            0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4, 8, 9, 10, 10, 11, 8, 12, 13, 14, 14, 15, 12, 16,
            17, 18, 18, 19, 16, 20, 21, 22, 22, 23, 20,
        ];

        Self { vertices, indices }
    }
}
