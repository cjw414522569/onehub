project(
  'ssh-client-linux-boundary',
  'c',
  version: '0.1.0',
  default_options: ['warning_level=3', 'werror=true']
)

boundary = executable(
  'ssh-client-linux-boundary',
  'src/boundary.c',
  install: false
)

test('linux-boundary-smoke', boundary)
