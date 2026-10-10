---
name: video-as-code
description: Create and update Python scenarios demonstrating niri features in tools/video-as-code. Use when recording reproducible footage of real UI and exporting MP4, WebP, or GIF.
---

# video-as-code scenario rules

- Demonstrate one observable behavior per scenario. Define the action, expected result, and evidence before writing code.
- Plan the complete sequence from the normal baseline through activation, intermediate states, and return. Do not omit transitions needed to understand the feature.
- Exercise the intended mechanism with real interactions. Do not substitute configuration changes or post-processing that merely resemble the result.
- Use small, typed Python scenarios based on the [existing scenarios](scenarios/) and [shared API](src/niri_video/). Reuse session, input, capture, and export primitives rather than introducing another framework or CLI.
- Check runtime and capture dependencies before starting. Compose configuration through supported mechanisms, validate it, and confirm that it was applied.
- Run in an isolated session with demo data. Route input and IPC only to that session; leave the user's desktop, configuration, and applications untouched.
- Establish predictable application and input state before recording. Diagnose unexpected behavior from logs and observed state instead of adding workaround shortcuts or longer delays.
- Choose content and framing that remain informative throughout the sequence. Keep controls, pointer motion, and results readable at the intended display size.
- Derive coordinates from session geometry and shared UI helpers. Keep interactions inside the frame and specify motion timing explicitly.
- Perform every action the viewer needs to see inside the recording scope, including the complete input lifecycle required by the feature.
- Wait for observable readiness and verify each state transition. Use pauses for viewing pace, not as substitutes for readiness checks.
- Keep calibration and reference comparisons outside the recording so they do not alter the demonstrated sequence.
- For looping previews, restore the initial view, pointer position, and UI state through recorded actions. A repeat flag or editing trick does not make a discontinuous sequence seamless.
- Verify behavior using both application state and visible results. A successful command, size change, or elapsed delay alone is not proof.
- Check loop boundaries in both the source recording and the encoded asset. With unchanged content, compare decoded first and last frames pixel for pixel, not compressed file bytes.
- Inspect intermediate frames and final playback at full resolution and at the publication size. Automated checks do not replace visual review.
- Run formatting, linting, type checks, and the actual scenario. Export only after the scenario succeeds; report failures rather than publishing incomplete demonstrations.
- Account for timeouts across the full execution chain. Wait for recording and export jobs to finish, including background jobs, before consuming their outputs.
- Export replacements to a temporary destination, verify them, then publish atomically. Keep the previous artifact and source recording until the replacement is accepted.
- Use the requested format and existing presentation layout. Keep captions brief and focused on the action and result; update the scenario, checks, and caption together when requirements change.
- Verify links, loading, layout, and builds after all media is published. Do not hide unrelated warnings or expand the task to fix them.
- Close owned sessions, clients, recorders, servers, and browsers; remove temporary profiles and scratch files, including after interrupted runs.
- Deliver a short summary with artifact links and observed verification. Include reproduction commands when useful and state any remaining limitation explicitly.
