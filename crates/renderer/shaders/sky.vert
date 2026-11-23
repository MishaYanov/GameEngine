#version 450
layout(location = 0) out vec2 screen_ndc;

void main() {
    const vec2 points[3] = vec2[3](vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    screen_ndc = points[gl_VertexIndex];
    gl_Position = vec4(screen_ndc, 1.0, 1.0);
}
