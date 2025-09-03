# Backend

## How to run

The backend is a Go application that uses the Echo framework. To run properly, you need to have a Postgres database and a Redis database (redis is used for pubsub between users for events like call notifications etc).

## How to run

If you run the backend for the first time, create certs for websockets to be able to operate in an HTTPS environment (webkit needs it). To create the certs we use [mkcert](https://github.com/FiloSottile/mkcert).

```
task create-certs
```

Run the databases and related services:

```
task compose-up
```

The to run a local server, which compiles the code and starts the server:

```
task dev
```

## Type-safe code generation

The backend uses [OpenAPI](https://swagger.io/docs/specification/about/) to define the API. We use [openapi-ts](https://github.com/openapi-ts/openapi-typescript) to generate type-safe code from the OpenAPI specification.

To do this from, we update the OpenAPI specification and run the following command inside the root directory:

```
yarn generate-openapi-types
```
