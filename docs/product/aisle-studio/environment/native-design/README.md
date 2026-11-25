# Native environment-workflow design review

**91/100**, internal Codex review of the implemented Makepad surfaces. The comparison uses the [generated design board and its separate review](../design-review.md). Both reviews use the ten PRD criteria. The native review used the actual populated application at 1024 × 768 and 1440 × 900.

| Criterion | Points | Observed evidence or deduction |
| --- | ---: | --- |
| Shell consistency | 10 | All ten captures retain the same four page names, active navigation indicator and button vocabulary. |
| Page identity | 10 | Each capture has a distinct work-area heading, including separate survey and scene-editing modes. |
| Next-action clarity | 9 | Save asset, save scene, use task and start batch are prominent. In Evidence the variant action requires scrolling past notes. |
| Robot identity | 9 | All five work areas identify Nano. The task view highlights the target geometry, while the robot model itself is in a separate Scene mode. |
| Simulation context | 9 | The UI distinguishes the source photo, authored room, onboard recording and Nexus result. Run repeats the engine name in its context line. |
| State visibility | 9 | Run shows all four recording combinations and a cancellation. Evidence shows the selected frame, measured trace and parent. The editable variant feedback sits below the initial viewport. |
| Task and result separation | 10 | The task binds a target and a receiver. The evidence inspector separates actual steps and contacts from editable notes and configurations. |
| Typography and readability | 8 | Primary labels and metric fields are legible at both sizes. Metadata text is small and long run names can truncate in the narrow selector. |
| Grouping and spacing | 8 | Camera or scene geometry stays next to the corresponding inspector. The narrow Scene view allocates few pixels to small objects in the full-room view. Survey and Run need inspector scrolling at 768 px high. |
| Artifact discoverability | 9 | The survey shows photo thumbnails. Evidence lists recorded filenames and the source trial ID. Full recording options in Run are lower in its inspector. |
| **Total** | **91** | The threshold in the PRD is 90. |

| Work area | 1024 × 768 | 1440 × 900 |
| --- | --- | --- |
| Photo survey and measured asset | [Narrow](survey-1024.png) | [Wide](survey-1440.png) |
| Scene composition | [Narrow](scene-1024.png) | [Wide](scene-1440.png) |
| Task object and surface bindings | [Narrow](task-1024.png) | [Wide](task-1440.png) |
| Repeat experiment and recording setup | [Narrow](run-1024.png) | [Wide](run-1440.png) |
| Trial playback and parent comparison | [Narrow](evidence-1024.png) | [Wide](evidence-1440.png) |

The instrumented check resized the native window, read the dimensions of each rendered PNG and checked the listed main controls against the actual window bounds. The numerical grade is a visual judgment. The size check verifies geometry. The five end-to-end checks in the [PRD](../../PRD.md#end-to-end-release-acceptance) verify behavior. [Measured control rectangles](window-sizes.json)

To reproduce, supply populated project sessions from the survey, experiment and trial-evidence checks.

```sh
python3 tools/check_environment_sizes.py --studio target/debug/aisle-studio \
  --remote "$(command -v makepad-remote)" --survey-session "$SURVEY_SESSION" \
  --experiment-session "$EXPERIMENT_SESSION" --evidence-session "$EVIDENCE_SESSION" \
  --output /tmp/aisle-native-window-review
```
