import { App, VStack, Text } from 'perry/ui';
import { getLevelInfo } from './test_android_labels';

const result = getLevelInfo(50);
console.log(result);

App({ title: "Test", width: 400, height: 300 }, () => {
    VStack(() => {
        Text(result);
    });
});
