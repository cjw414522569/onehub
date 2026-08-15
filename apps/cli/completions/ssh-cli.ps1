# PowerShell completion for ssh-cli (T145)
Register-ArgumentCompleter -Native -CommandName ssh-cli -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    $commands = @('--version', '--help', '--json', 'config', 'cap')
    $commands | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
    }
}