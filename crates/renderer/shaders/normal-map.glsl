const uint NORMAL_MAP_FLAG = 1024u;

vec4 modelTangent(mat3 linear, float determinantValue, vec4 tangent) {
    if (abs(determinantValue) <= 1e-12 || isinf(determinantValue) || isnan(determinantValue)) return vec4(0.0);
    return vec4(linear * tangent.xyz, tangent.w * sign(determinantValue));
}

vec3 mappedNormal(vec3 normal, vec4 tangent, vec3 encoded, float strength) {
    // Remove interpolation and nonuniform scaling shear from the basis.
    vec3 axis = tangent.xyz - normal * dot(normal, tangent.xyz);
    float axisLen2 = dot(axis, axis);
    if (axisLen2 <= 1e-12 || isinf(axisLen2) || isnan(axisLen2)) return normal;
    axis *= inversesqrt(axisLen2);
    vec3 bitangent = cross(normal, axis) * sign(tangent.w);
    vec3 sampleNormal = encoded * 2.0 - 1.0;
    sampleNormal.xy *= strength;
    vec3 world = mat3(axis, bitangent, normal) * sampleNormal;
    float len2 = dot(world, world);
    return len2 > 1e-12 && !isinf(len2) ? world * inversesqrt(len2) : normal;
}
