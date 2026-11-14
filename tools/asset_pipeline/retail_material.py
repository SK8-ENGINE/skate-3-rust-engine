"""Retail shader classification shared with the supplied map exporter."""
def _retail_shader_family(shader_name: str) -> int:
    shader = shader_name.lower()
    if shader.startswith("environment.reflective_simple"):
        return 6
    if shader.startswith("environment.reflective_trans"):
        return 13
    if shader.startswith("environment.reflective"):
        return 5
    if shader.startswith("environment.decal_tileable"):
        return 4
    if shader.startswith("environment.decal"):
        return 3
    if shader.startswith("environment.default"):
        return 1
    if shader == "environment.transparent":
        # transparentenvironment_defaultPS: alpha-scaled lightmapped diffuse,
        # alpha squared out (chain-link fences, wire mesh).
        return 16
    if shader == "advertisement.default":
        # advertisement_defaultPS: diffuse^2 times the boxed, shadowed lightmap
        # with a global light floor; no kd, opaque, the reduced output curve.
        return 17
    # The skate.Park pieces' programs (research b442): environmentpark{diffuse,decal,alphatest}_defaultPS.
    if shader.startswith("environmentpark.diffuse"):
        return 50
    if shader.startswith("environmentpark.decal"):
        return 51
    if shader.startswith("environmentpark.alphatest"):
        return 52
    if shader.startswith("environmentsimple.alphatest"):
        return 7
    if shader.startswith("environmentsimple.diffuse"):
        return 8
    if shader.startswith("environmentsimple.default"):
        return 2
    if shader.startswith("tree.default"):
        return 9
    if shader == "animated.flag":
        # vertexanimate_defaultPS / VS: lightmapped diffuse scaled by
        # g_ViewDotLight.x, alpha-tested, cloth sway in the vertex stage.
        return 21
    if shader.startswith("animated.tree"):
        return 10
    if shader.startswith("proxyworld."):
        return 11
    if shader.startswith("incandescent.backlituvscroll"):
        return 14
    if shader == "incandescent.transparent":
        # transparentincandescent_defaultPS: diffuse^2 * m_params, alpha out,
        # blended with depth write (lit signs).
        return 18
    if shader == "trafficlight.one":
        # trafficlight_one_defaultPS / VS: unsquared diffuse; the lamp state picks
        # each vertex's UV set (g_TrafficLightsStatus_1).
        return 19
    if shader == "trafficlight.two":
        return 20
    if shader.startswith("incandescent.default") or shader == "incandescent.videoscreen":
        # videoscreen_defaultPS is the same program as baseincandescent_defaultPS;
        # only its m_params.y differs (0.25).
        return 12
    if shader.startswith("water.flowing"):
        return 30
    if shader in ("water.default", "water.alpha", "water.skatepark"):
        return 33
    if shader.startswith("ocean.default"):
        return 31
    if shader.startswith("ocean.reflection"):
        return 32
    if shader.startswith("sky."):
        return 40
    if shader.startswith("dynamicobject."):
        # Props (DMOs): dynamicobject_defaultPS, sun + shadow lit, no lightmap.
        return 15
    return 0

def _retail_render_flags(shader_name: str, alpha_mode: int) -> int:
    shader = shader_name.lower()
    flags = 0
    if alpha_mode == 1:
        flags |= 1
    elif alpha_mode == 2:
        flags |= 2
    if (
        shader.startswith(("tree.", "animated.tree"))
        or "alphatest" in shader
    ):
        flags |= 1 | 4
    if shader.startswith("sky."):
        flags |= 8
    if shader.startswith("environment.decal"):
        flags |= 16
    if shader.startswith("environment.decal_tileable"):
        flags |= 32
    if shader.startswith(("water.", "ocean.")):
        flags |= 64
    return flags
