<?php

namespace App\Providers;

use App\Events\UserRegistered;
use App\Listeners\Orphaned;
use App\Listeners\SendWelcomeEmail;
use Illuminate\Support\ServiceProvider;

class EventServiceProvider extends ServiceProvider
{
    protected $listen = [
        UserRegistered::class => [
            SendWelcomeEmail::class,
            Orphaned::class,
        ],
    ];
}
