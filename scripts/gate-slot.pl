#!/usr/bin/env perl
# Machine-wide limit for heavy gate steps (#185): `perl scripts/gate-slot.pl <command...>` waits for one of
# KELTA_GATE_SLOTS (default 2) lock files under ${TMPDIR}/kelta-gate/, runs the command while holding it and
# exits with its status. flock(2) through perl: macOS has no flock(1). The lock is opened close-on-exec, so a
# process the command leaves behind cannot keep the slot; it is freed when this process exits or is killed.
# shortcut: the pool is per TMPDIR (per user on macOS); a lane with its own TMPDIR gets its own pool.
use strict;
use warnings;
use Fcntl qw(:flock);

my $dir = ($ENV{TMPDIR} || "/tmp") . "/kelta-gate";
my $n = $ENV{KELTA_GATE_SLOTS} || 2;
die "gate-slot: KELTA_GATE_SLOTS must be a positive integer\n" unless $n =~ /^[1-9][0-9]*$/;
die "gate-slot: no command\n" unless @ARGV;
mkdir $dir;
my $t0 = time;
while (1) {
    for my $i (1 .. $n) {
        open(my $lock, ">>", "$dir/slot-$i") or die "gate-slot: $dir/slot-$i: $!\n";
        next unless flock($lock, LOCK_EX | LOCK_NB);
        printf STDERR "gate-slot: slot %d/%d after %ds wait: %s\n", $i, $n, time - $t0, join(" ", @ARGV);
        system { $ARGV[0] } @ARGV;
        exit($? == -1 ? 127 : $? & 127 ? 128 + ($? & 127) : $? >> 8);
    }
    sleep 1;
}
