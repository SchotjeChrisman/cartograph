<?php

use App\Greeter;
use App\Util\Formatter;
use Vendor\Package\Thing;

function main($anything): void
{
    $greeter = new Greeter(Formatter::make());
    echo $greeter->greet('world');
    echo $anything->format('untyped');
    new Thing();
}

main(null);
