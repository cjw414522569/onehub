#compdef ssh-cli
# zsh completion for ssh-cli (T145)
_ssh_cli() {
    local -a commands
    commands=(
        '--version:Show version'
        '--help:Show help'
        '--json:Machine-readable JSON output'
        'config:Config commands'
        'cap:Capability commands'
    )
    _describe 'command' commands
}
compdef _ssh_cli ssh-cli