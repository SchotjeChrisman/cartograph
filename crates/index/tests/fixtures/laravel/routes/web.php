<?php

use App\Http\Controllers\OrderController;
use App\Http\Controllers\UserController;
use Illuminate\Support\Facades\Route;

Route::get('/', function () {
    return view('welcome');
});

Route::get('/users', [UserController::class, 'index']);
Route::post('/users', [UserController::class, 'store']);

Route::prefix('admin')->group(function () {
    Route::get('/users/{user}', [UserController::class, 'show']);
    Route::get('/audit', [UserController::class, 'audit']);
});

Route::resource('orders', OrderController::class);

Route::get('/legacy', 'UserController@index');
Route::get('/gone', [App\Http\Controllers\GhostController::class, 'index']);
