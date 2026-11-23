// Defines a second helper(), so a bare helper() call elsewhere is ambiguous.
static int helper() { return 2; }

int other_entry() { return helper(); }
