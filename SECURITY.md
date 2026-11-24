# Security

Please report a vulnerability privately, through **Security › Report a vulnerability** on
https://github.com/mmazzieri/musiccraft, not in a public issue.

What matters most here: files shared through a sync service are treated as hostile input (paths that
would leave the music folder are refused, sizes are limited), tags and pictures come from files of
unknown origin, and passwords and tokens are kept only in the system's credential store.
