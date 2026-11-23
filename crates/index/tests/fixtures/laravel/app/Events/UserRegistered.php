<?php

namespace App\Events;

class UserRegistered
{
    public function __construct(public $user)
    {
    }
}
