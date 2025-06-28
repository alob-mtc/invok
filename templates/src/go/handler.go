package main

import (
    "net/http"
)

// Handler for the "/{{ROUTE}}" endpoint.
func {{HANDLER}}(w http.ResponseWriter, r *http.Request) {
    // You can access query params via r.URL.Query().
    // For example:
    // query := r.URL.Query()
    // name := query.Get("name")

	w.WriteHeader(http.StatusOK)
	w.Write([]byte("Hello World!"))
}