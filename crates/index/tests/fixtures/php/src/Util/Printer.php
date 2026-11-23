<?php

namespace App\Util;

class Printer
{
    // Also named `format`, so a call on an untyped value is ambiguous.
    public function format(string $text): string
    {
        return $text;
    }
}
