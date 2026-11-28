#version 450
layout(location = 0) in vec2 ndc;
layout(location = 0) out vec4 outColor;
layout(push_constant, std430) uniform GridData {
    mat4 inverseViewProjection;
    vec4 originSpacing;
    vec4 tangentOpacity;
    vec4 normalFade;
    vec4 viewport;
} grid;

float safeW(float value) {
    return abs(value) < 1e-8 ? (value < 0.0 ? -1e-8 : 1e-8) : value;
}

vec2 gridCoverage(vec2 uv, vec2 footprint, float halfWidth) {
    vec2 distance = abs(fract(uv - 0.5) - 0.5) / max(footprint, vec2(1e-6));
    vec2 line = 1.0 - smoothstep(vec2(0.1), vec2(halfWidth), distance);
    // Fade each direction independently as its cells become too small.
    return line * (1.0 - smoothstep(vec2(0.18), vec2(0.55), footprint));
}

vec3 axisColor(vec3 axis) {
    return abs(axis.x) > 0.5 ? vec3(0.69, 0.22, 0.21)
         : abs(axis.y) > 0.5 ? vec3(0.20, 0.58, 0.33) : vec3(0.21, 0.41, 0.76);
}

void main() {
    vec4 nearH = grid.inverseViewProjection * vec4(ndc, 0.0, 1.0);
    vec4 farH = grid.inverseViewProjection * vec4(ndc, 1.0, 1.0);
    vec3 nearPoint = nearH.xyz / safeW(nearH.w);
    vec3 farPoint = farH.xyz / safeW(farH.w);
    vec3 ray = farPoint - nearPoint;
    vec3 normal = grid.normalFade.xyz;
    float denominator = dot(normal, ray);
    float t = dot(normal, grid.originSpacing.xyz - nearPoint) / safeW(denominator);
    vec3 world = nearPoint + t * ray;
    vec3 tangent = grid.tangentOpacity.xyz;
    vec3 bitangent = cross(normal, tangent);
    vec3 relative = world - grid.originSpacing.xyz;
    vec2 uv = vec2(dot(relative, tangent), dot(relative, bitangent)) / grid.originSpacing.w;

    // Derivatives precede fragment-varying rejection, including at the horizon.
    vec2 footprint = max(fwidth(uv), vec2(1e-6));
    vec2 minor = gridCoverage(uv, footprint, 0.75);
    vec2 major = gridCoverage(uv / 10.0, footprint / 10.0, 1.0);
    float minorLine = 0.27 * max(minor.x, minor.y);
    float majorLine = 0.65 * max(major.x, major.y);
    float lineAlpha = max(minorLine, majorLine);
    vec3 color = mix(vec3(0.28, 0.33, 0.40), vec3(0.43, 0.49, 0.57),
                     majorLine / max(lineAlpha, 1e-6));
    vec2 axis = 1.0 - smoothstep(vec2(0.15), vec2(1.1), abs(uv) / footprint);
    color = mix(color, axisColor(tangent), axis.y);
    color = mix(color, axisColor(bitangent), axis.x);
    float alpha = max(lineAlpha, 0.92 * max(axis.x, axis.y));

    float distance = length(world - nearPoint);
    float distanceFade = 1.0 - smoothstep(grid.normalFade.w * 0.4, grid.normalFade.w, distance);
    float angleFade = smoothstep(0.02, 0.10, abs(denominator) / max(length(ray), 1e-6));
    float depthDenominator = farH.w + t * (nearH.w - farH.w);
    float depth = t * nearH.w / safeW(depthDenominator);
    if (abs(denominator) < 1e-6 || t < 0.0 || t > 1.0
        || depth < 0.0 || depth > 1.0 || isnan(depth) || isinf(depth)) discard;

    alpha *= grid.tangentOpacity.w * distanceFade * angleFade;
    if (alpha < 0.001) discard;

    // A fixed negative depth bias can pull distant grid lines through nearby geometry.
    gl_FragDepth = depth;
    outColor = vec4(color, alpha);
}
