# Environment and experiment design review

[ImageGen design board](design-board.png) · [Release acceptance](../PRD.md#end-to-end-release-acceptance)

Codex reviewed this desktop concept against the existing ten-part rubric on November 24, 2026. The reference supplied to ImageGen was the [standalone Nano window](../../../../media/nano-three-cubes.jpg). Scores apply to the concept. Runtime interaction, resizing, failure handling and persistence require separate native checks.

| Criterion | Points / 10 | Evidence and deductions |
| --- | ---: | --- |
| Shell consistency | 10 | All four views retain the standalone header, page order, type hierarchy and blue selection. |
| Page identity | 10 | Both Scene modes, Repeat experiment and the selected trial have distinct headings. |
| Next action | 9 | Save asset, Save revision, Start batch and Rerun original are prominent. The scene’s proposal review requires a separate state. |
| Robot identity | 8 | Nano appears in context throughout. The isometric robot is an approximation and the board omits G1 selection. |
| Simulation context | 10 | AISLE and Nexus are visible in each view. The source photo, measured scene and recorded head-camera image are distinct. |
| State visibility | 8 | The board shows selection, recording overrides and run identifiers. Validation errors and interrupted trials require additional states. |
| Task and result separation | 9 | Scene binds the cube and tabletop. Evidence puts comparison beside replay. A separate measured verdict is needed alongside editable notes. |
| Readability | 9 | Source, unit, seed and both recording channel labels remain legible. The smallest collider annotations need more room at narrow widths. |
| Grouping and spacing | 10 | Each editing viewport has an adjacent inspector. The batch table exposes all four recording choices. |
| Artifact discoverability | 8 | Photos, seeds, both recording options and the replay timeline are visible. The Evidence implementation must also show recorded filenames and missing channels. |
| **Total** | **91/100** | Internal concept review. |

The implementation uses the actual Nano robot model and the source photo selected by the operator. Example scenes and run values on the board are illustrative.
