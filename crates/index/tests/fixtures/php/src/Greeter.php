<?php

namespace App;

use App\Util\Formatter;
use function App\Util\slug;

interface Greets
{
    public function greet(string $name): string;
}

trait Loud
{
    public function shout(string $text): string
    {
        return strtoupper($text);
    }
}

abstract class Base
{
    public function prefix(): string
    {
        return 'Hello';
    }
}

class Greeter extends Base implements Greets
{
    use Loud;

    public function __construct(private Formatter $formatter)
    {
    }

    public function greet(string $name): string
    {
        $text = $this->prefix() . ' ' . slug($name);
        return $this->shout($this->formatter->format($text));
    }
}
