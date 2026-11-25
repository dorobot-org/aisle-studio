# Codex and DeepSeek/Octos on the same Nano task

Both providers received the same instruction, output schema and public feedback from disclosed layout 16. The request, with its provider label removed, and the feedback bytes have matching digests. Each provider prepared its own controller through the normal Task backend. On the same frozen release runtime, Nexus executed each controller on three previously disclosed layouts. The cases represent a placement failure, a missed grasp and a docking timeout. [Requests, timings, outcomes and six distinct receipts](comparison.json)

| Provider | Model | Preparation time, one call | Complete three-cube layouts |
| --- | --- | ---: | ---: |
| Codex | gpt-6.2-astra | 7.14 s | 2/3 |
| DeepSeek via Octos | deepseek-v4-flash | 2.36 s | 2/3 |

| Disclosed layout | Codex | DeepSeek via Octos | Identical motor journals |
| --- | --- | --- | --- |
| 016 | All three transferred | All three transferred | Yes |
| 020 | All three transferred | All three transferred | Yes |
| 043 | Stereo table docking timed out | Stereo table docking timed out | Yes |

Both authors selected red, green, blue order, stationary grasp and wrist clearance. The prepared JSON differs only in the author and policy-model fields. The local RGB/encoder controller then issued the motor commands. These three matched layouts test interchangeability for this instruction. Each preparation time is a single observation and the 50-layout release qualification belongs to its registered DeepSeek artifact.
