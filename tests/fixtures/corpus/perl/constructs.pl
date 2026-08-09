use strict;

sub greet {
    my ($name) = @_;
    print "Hello, $name
";
    return $name;
}

my $counter = 42;
if ($counter > 10) {
    $counter = $counter + 1;
}
