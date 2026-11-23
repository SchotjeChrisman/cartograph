<?php

namespace App\Services;

use App\Contracts\PaymentGateway;

class StripeGateway implements PaymentGateway
{
    public function charge(int $cents): bool
    {
        return config('services.stripe.key') !== null;
    }
}
