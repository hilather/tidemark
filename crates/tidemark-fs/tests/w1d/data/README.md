# Procfs fixtures

stat.txt, fdinfo.txt and the trimmed Name/Uid status.txt were captured from a short-lived Python process on the worker's Linux host on 2026-10-11. fdinfo describes an O_WRONLY|O_APPEND descriptor in a temporary directory. No regular-file contents were read.

cgroup-v1.txt and cgroup-v2.txt are representative systemd hierarchy fixtures based on the documented Linux layout; they are not claimed as Rocky VM captures. The parser test also supplies a constructed comm containing spaces and both kinds of parentheses. Real Rocky 8 v1 captures remain pending VM.
