import "reflect-metadata";
import { Controller, Get, Injectable, Module } from "@nestjs/common";
import { NestFactory } from "@nestjs/core";

@Injectable()
class AppService {
    getHello(): string {
        return "Hello Perry";
    }
}

@Controller()
class AppController {
    constructor(private readonly appService: AppService) {}

    @Get()
    getHello(): string {
        return this.appService.getHello();
    }
}

@Module({
    imports: [],
    controllers: [AppController],
    providers: [AppService],
})
class AppModule {}

async function bootstrap() {
    const port = Number(process.env.PERRY_NEST_PORT ?? "13754");
    const app = await NestFactory.create(AppModule, { logger: false });
    await app.listen(port);
    // Tell the fixture driver we're up. Use a fixed marker so the driver
    // can wait on it rather than polling the port. Print to stdout (the
    // fixture redirects stdout to a log + watches that log).
    console.log(`nestjs-hello listening on ${port}`);
}

bootstrap();
