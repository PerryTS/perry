package com.perry.app;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.os.Bundle;
import android.os.Looper;
import android.os.SystemClock;
import android.view.View;
import android.view.ViewGroup;
import android.widget.LinearLayout;
import android.widget.TextView;

public final class TableTest extends Instrumentation {
    private static native long query(int kind,long index);
    private static native void update(int kind,long value);
    private static int selectionCallbacks,sortCallbacks,lastRow=-100,lastColumn=-1;
    private static double ascending=-1;
    private static boolean callbackStateMatches,renderOnUi=true;
    private View root;
    public static View renderCell(int row,int col) {
        renderOnUi &= Looper.myLooper()==Looper.getMainLooper();
        TextView text=new TextView(PerryBridge.getActivity());
        text.setText("r"+row+"c"+col);
        if(row==0 && col==1) {
            LinearLayout composite=new LinearLayout(PerryBridge.getActivity());
            composite.setOrientation(LinearLayout.VERTICAL);
            composite.addView(text);
            TextView detail=new TextView(PerryBridge.getActivity());detail.setText("DETAIL");composite.addView(detail);
            return composite;
        }
        return text;
    }
    public static void selected(int row) {
        selectionCallbacks++;lastRow=row;
        callbackStateMatches=query(1,0)==row; // Native getter re-entry on the UI callback thread.
    }
    public static void sorted(int col,double asc) {
        sortCallbacks++;lastColumn=col;ascending=asc;
        update(0,3); // User sort handlers can re-render cells synchronously.
    }
    @Override public void onCreate(Bundle args) {super.onCreate(args);start();}
    private static void check(boolean ok,String why) {if(!ok)throw new AssertionError(why);}
    private TextView find(View view,String text) {
        if(view instanceof TextView && ((TextView)view).getText().toString().equals(text))return (TextView)view;
        if(view instanceof ViewGroup)for(int i=0;i<((ViewGroup)view).getChildCount();i++) {
            TextView found=find(((ViewGroup)view).getChildAt(i),text);if(found!=null)return found;
        }
        return null;
    }
    private TextView text(String text) {TextView v=find(root,text);check(v!=null,"missing "+text);return v;}
    private View cell(int row,int col) {
        View view=text("r"+row+"c"+col);
        if(row==0&&col==1)view=(View)view.getParent();
        return (View)view.getParent();
    }
    private void ui(Runnable run) {
        Throwable[] error=new Throwable[1];
        runOnMainSync(()->{try{run.run();}catch(Throwable e){error[0]=e;}});
        if(error[0]!=null)throw new AssertionError(error[0]);
    }
    private void settle(){SystemClock.sleep(200);waitForIdleSync();}
    @Override public void onStart() {
        Bundle result=new Bundle();
        try {
            Intent intent=new Intent(getTargetContext(),PerryActivity.class);intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            Activity activity=startActivitySync(intent);root=activity.getWindow().getDecorView();
            for(int i=0;i<30&&query(0,0)==0;i++)settle();
            check(query(0,0)>0,"Table returned the stub handle 0");settle();
            ui(()->{
                check(query(4,0)==12,"expected 12 render callback calls");
                check(renderOnUi,"render callback ran outside UI thread");
                check(text("DETAIL").getHeight()>0,"composite cell was not attached/measured");
                for(int col=0;col<4;col++)for(int row=0;row<3;row++) {
                    View c=cell(row,col);View h=text("Header "+col);
                    check(c.getWidth()>0&&c.getHeight()>0,"cell has no area");
                    check(c.getLeft()==h.getLeft()&&c.getWidth()==h.getWidth(),"header/body columns misaligned");
                }
                check(query(1,0)==-1&&query(2,0)==0,"initial selection wrong");
                cell(1,0).performClick();
                check(lastRow==1&&callbackStateMatches&&selectionCallbacks==1,"selection callback/getter wrong");
            });
            settle();
            android.graphics.Bitmap screenshot=getUiAutomation().takeScreenshot();
            try(java.io.FileOutputStream image=new java.io.FileOutputStream(
                    new java.io.File(getTargetContext().getFilesDir(),"table.png"))) {
                screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,image);
            }
            // Native-thread getters/setters after attachment.
            check(query(1,0)==1,"selection not visible from native thread");
            update(1,1);
            ui(()->{
                cell(2,0).performClick();
                check(query(2,0)==2&&query(3,0)==1&&query(3,1)==2,"multi-selection wrong");
                check(query(3,-1)==-1&&query(3,2)==-1&&query(3,Long.MAX_VALUE)==-1,"invalid selection index accepted");
                cell(1,0).performClick();
                check(query(1,0)==2&&query(2,0)==1,"multi-select toggle wrong");
            });
            update(0,2);settle();
            ui(()->{
                check(find(root,"r2c0")==null,"removed row still attached");
                check(query(1,0)==-1&&query(2,0)==0,"removed selection survived");
                check(query(4,0)==20,"row update did not re-render cells");
            });
            update(2,0);check(query(5,0)==1,"Unicode filter did not round trip via string ABI");
            update(3,170);settle();
            ui(()->{
                check(cell(0,1).getWidth()==text("Header 1").getWidth(),"updated width lost header alignment");
                check(cell(0,1).getWidth()==cell(1,1).getWidth(),"updated width lost row alignment");
                text("Header 2").performClick();
                check(sortCallbacks==1&&lastColumn==2&&ascending==1,"initial sort callback wrong");
                text("Header 2").performClick();
                check(sortCallbacks==2&&ascending==0,"sort direction did not toggle");
                check(query(4,0)==44,"sort callback could not re-enter row rendering");
                cell(0,0).performClick();cell(1,0).performClick();
            });
            update(1,0);check(query(2,0)==1&&query(1,0)==0,"disabling multiple selection failed");
            update(0,0);settle();
            ui(()->check(find(root,"r0c0")==null&&query(1,0)==-1,"empty table retained rows/selection"));
            update(0,1);settle();
            ui(()->check(cell(0,1).getWidth()==text("Header 1").getWidth(),"repopulation lost column width"));
            update(0,30);settle();
            ViewGroup grid=(ViewGroup)text("Header 0").getParent().getParent();
            android.widget.HorizontalScrollView horizontal=(android.widget.HorizontalScrollView)grid.getParent();
            android.widget.ScrollView vertical=(android.widget.ScrollView)grid.getChildAt(1);
            ui(()->{horizontal.scrollTo(Integer.MAX_VALUE,0);vertical.scrollTo(0,Integer.MAX_VALUE);});
            settle();
            ui(()->check(horizontal.getScrollX()>0&&vertical.getScrollY()>0,"table did not scroll in both axes"));
            result.putString("stream","\nPASS: production Rust Table ABI, widget cells, aligned columns, selection/multi-selection, updates, sort re-entry, Unicode filter, two-axis scrolling\n");finish(0,result);
        }catch(Throwable error){result.putString("stream","\nFAIL: "+android.util.Log.getStackTraceString(error));finish(1,result);}
    }
}
