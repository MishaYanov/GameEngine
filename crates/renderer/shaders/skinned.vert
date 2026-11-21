#version 450
layout(location=0) in vec3 inPosition;
layout(location=1) in vec2 inUv;
layout(location=2) in vec3 inNormal;
layout(location=3) in uvec4 inJoints;
layout(location=4) in vec4 inWeights;
layout(location=0) out vec2 outUv;
layout(location=1) out vec3 outNormal;
layout(location=2) out vec3 outWorldPosition;
layout(location=3) out vec4 outTint;
layout(set=0,binding=0) uniform CameraUniform { mat4 viewProjection; } camera;
layout(set=2,binding=0,std430) readonly buffer SkinPalette { mat4 joints[]; } skin;
// Exactly the same 96-byte layout as triangle.vert and triangle.frag.
// flags.x remains material/render flags; y/z are palette base/count.
layout(push_constant,std430) uniform ModelPushConstants {
    mat4 model; vec4 baseColorFactor; uvec4 flags;
} modelData;
void main() {
    vec4 p = vec4(0); vec3 normal = vec3(0); vec3 dominant = inNormal; float highest = -1;
    for (int i=0;i<4;i++) {
        uint index = modelData.flags.y + min(inJoints[i], modelData.flags.z-1u);
        mat4 joint = skin.joints[index];
        p += inWeights[i] * (joint * vec4(inPosition,1));
        // Each palette entry is a checked positive similarity. Removing its
        // uniform scale avoids inverting the potentially singular LBS blend.
        vec3 n = mat3(joint) * inNormal;
        n = normalize(n);
        normal += inWeights[i] * n;
        if (inWeights[i] > highest) { highest = inWeights[i]; dominant = n; }
    }
    // Opposite valid rotations may cancel. Stable largest-weight fallback;
    // ties select the first influence, independent of GPU scheduling.
    normal = dot(normal,normal) > 1e-12 ? normalize(normal) : dominant;
    vec4 world = modelData.model * p;
    gl_Position = camera.viewProjection * world;
    outWorldPosition = world.xyz;
    outNormal = normalize(mat3(modelData.model) * normal);
    outUv = inUv; outTint = modelData.baseColorFactor;
}
