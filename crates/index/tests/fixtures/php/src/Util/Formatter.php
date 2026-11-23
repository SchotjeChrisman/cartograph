<?php

namespace App\Util;

class Formatter
{
    public function format(string $text): string
    {
        return trim($text);
    }

    public static function make(): self
    {
        return new self();
    }
}
