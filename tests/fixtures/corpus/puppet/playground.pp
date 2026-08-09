class greeting (
  String $message = 'Hello, World!',
  String $target   = 'console',
) {
  notify { 'hello':
    message => $message,
  }

  file { '/tmp/greeting.txt':
    ensure  => present,
    content => $message,
  }
}
