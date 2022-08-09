#!/usr/bin/env bash

sudo docker run -d --name perf-db -p 127.0.0.1:5432:5432 -e POSTGRES_USER='prusti' -e POSTGRES_PASSWORD='prusti' postgres 
