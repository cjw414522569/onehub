# fish completion for ssh-cli (T145)
complete -c ssh-cli -f
complete -c ssh-cli -n '__fish_use_subcommand' -a '--version' -d 'Show version'
complete -c ssh-cli -n '__fish_use_subcommand' -a '--help' -d 'Show help'
complete -c ssh-cli -n '__fish_use_subcommand' -a '--json' -d 'Machine-readable JSON output'
complete -c ssh-cli -n '__fish_use_subcommand' -a config -d 'Config commands'
complete -c ssh-cli -n '__fish_use_subcommand' -a cap -d 'Capability commands'
complete -c ssh-cli -n '__fish_seen_subcommand_from config' -a '--check' -d 'Validate a config file'
complete -c ssh-cli -n '__fish_seen_subcommand_from cap' -a 'forward sftp proxy' -d 'Capability commands'