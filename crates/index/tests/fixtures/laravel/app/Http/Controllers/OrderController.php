<?php

namespace App\Http\Controllers;

use Illuminate\Support\Facades\DB;

class OrderController
{
    public function index()
    {
        return DB::table('orders')->get();
    }

    public function store()
    {
        DB::table('orders')->insert(['total' => 1]);
    }
}
