use strict;
use warnings;

sub greet {
    my ($name) = @_;
    print "Hello, ${name}!\n";
}

sub add {
    my ($x, $y) = @_;
    return $x + $y;
}
