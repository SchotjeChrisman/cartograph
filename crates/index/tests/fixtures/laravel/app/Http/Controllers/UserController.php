<?php

namespace App\Http\Controllers;

use App\Events\UserRegistered;
use App\Models\User;

class UserController
{
    public function index()
    {
        $theme = config('shop.theme');
        return view('users.index', ['users' => User::all(), 'theme' => $theme]);
    }

    public function store()
    {
        $user = User::create(['name' => 'new']);
        event(new UserRegistered($user));
        return view('users.show');
    }

    public function show(User $user)
    {
        return view('users.show', compact('user'));
    }
}
